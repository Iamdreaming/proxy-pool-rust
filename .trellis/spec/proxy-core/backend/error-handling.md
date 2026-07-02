# Error Handling

How errors are defined, propagated, and logged in `proxy-core`.

---

## Current State

`proxy-core` currently uses **`anyhow::Result`** exclusively for fallible public APIs.
`thiserror` is listed as a dependency in `Cargo.toml` but **not yet used** in any
source file. The project CLAUDE.md prescribes `thiserror` for library code and
`anyhow` for application code — this crate should migrate toward that pattern.

---

## Error Type Strategy

| Context | Use | Rationale |
|---------|-----|-----------|
| Library public API (store, geoip, router) | `thiserror` enum | Callers need to match on specific failure modes |
| Application/wiring code (scheduler, fetcher) | `anyhow::Result` | Errors are logged and propagated; no programmatic matching needed |
| Fetcher implementations | Return empty `Vec<Proxy>` on error | Fetchers are best-effort; a failed source should not crash the pipeline |
| Config loading | Return defaults on error | `load_settings()` never fails — missing/invalid YAML falls back gracefully |

---

## Error Propagation Patterns

### Store methods — `anyhow::Result` (current, to migrate to `thiserror`)

```rust
// store.rs:90
pub async fn add(&self, proxy: &Proxy) -> anyhow::Result<()> {
    let s = score(proxy, &self.weights);
    let member = serde_json::to_string(proxy)?;       // serde error → anyhow
    let key = redis_key(&proxy.protocol);
    let mut conn = self.conn();
    let _: () = conn.zadd(&key, &member, s).await?;   // redis error → anyhow
    Ok(())
}
```

Errors from `serde_json` and `redis` are propagated via `?` into `anyhow::Result`.
When migrating to `thiserror`, define a `StoreError` enum with `Serialization` and
`Redis` variants.

### Scheduler — `anyhow::Result` (correct as-is)

```rust
// scheduler.rs:42-49
pub async fn refresh(&self) -> anyhow::Result<SchedulerResult> {
    let (tx, rx) = oneshot::channel();
    self.cmd_tx
        .send(SchedulerCommand::Refresh { reply: tx })
        .await
        .map_err(|_| anyhow::anyhow!("scheduler channel closed"))?;
    rx.await
        .map_err(|_| anyhow::anyhow!("scheduler result dropped"))
}
```

Channel errors are converted to descriptive `anyhow` errors. This is correct —
the caller only needs to know "refresh failed", not which internal channel broke.

### Config loading — never fails

```rust
// config.rs:265-290
pub fn load_settings(path: impl AsRef<Path>) -> Settings {
    let path = path.as_ref();
    if !path.exists() {
        return Settings::default();
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("cannot read config file {}: {e}, using defaults", path.display());
            return Settings::default();
        }
    };
    match serde_yaml::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("invalid config file {}: {e}, using defaults", path.display());
            Settings::default()
        }
    }
}
```

Config errors are **swallowed with a log + fallback**. This is intentional — the
service must start even with a broken config file.

### Fetcher — errors become empty results

```rust
// fetcher/proxyscrape.rs:28-74
async fn fetch(&self) -> Vec<Proxy> {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(self.timeout_secs))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("{}: build client failed: {e}", self.name());
            return Vec::new();
        }
    };
    // ... same pattern for send() and text()
}
```

Fetchers **never return `Result`**. Errors are logged at `warn` level and an empty
vec is returned. The `Fetcher` trait signature enforces this:

```rust
// fetcher/base.rs:7
async fn fetch(&self) -> Vec<Proxy>;
```

---

## Planned `thiserror` Migration for Store

When `ProxyStore` methods are migrated to `thiserror`, the error enum should be:

```rust
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("redis operation failed: {0}")]
    Redis(#[from] redis::RedisError),
}
```

Public methods would return `Result<T, StoreError>` instead of `anyhow::Result<T>`.
Internal helper `remove_existing` would use the same type.

---

## Forbidden Patterns

| Pattern | Why | Fix |
|---------|-----|-----|
| `unwrap()` in non-test code | Panics in production | Use `?`, `ok()?`, or `map_err` |
| Silent error swallowing without logging | Hides failures | Always log at `warn` or `error` before discarding |
| `Box<dyn std::error::Error>` | Opaque, unmatchable | Use `thiserror` enum or `anyhow::Result` |
| `panic!()` for validation | Crashes the process | Return `Result` or use graceful fallback |
| `.expect("...")` in library code | Panics with a message | Same as `unwrap()` — use `?` |

---

## Common Mistakes

1. **Forgetting to log before returning empty/default**: Every error-to-empty
   conversion must have a `tracing::warn!` or `tracing::error!` call. See
   `fetcher/proxyscrape.rs:35-37` for the correct pattern.

2. **Using `anyhow` in a library where callers need to match**: `ProxyStore`
   methods currently return `anyhow::Result`, making it impossible for
   `proxy-gateway` to distinguish "Redis down" from "bad proxy JSON". Migrate
   to `thiserror`.

3. **Not propagating the original error**: When wrapping with `anyhow::anyhow!`,
   the original error chain is lost. Use `anyhow::Context` instead:
   ```rust
   // Bad:
   .map_err(|_| anyhow::anyhow!("scheduler channel closed"))?;
   // Acceptable for channel errors (no source to preserve), but for I/O:
   // Good:
   .context("failed to connect to Redis")?;
   ```
