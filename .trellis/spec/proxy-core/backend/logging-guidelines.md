# Logging Guidelines

How `tracing` is used in `proxy-core`.

---

## Logging Library

**`tracing`** with structured fields. The `log` crate is forbidden.

All macros come from `tracing`: `tracing::info!`, `tracing::warn!`,
`tracing::error!`, `tracing::debug!`.

---

## Log Level Conventions

| Level | When to use | Examples in codebase |
|-------|-------------|----------------------|
| `error!` | Unrecoverable condition; service degraded but continues | `config.rs:283` — invalid config file, falling back to defaults; `geoip.rs:33` — GeoIP database failed to load |
| `warn!` | Recoverable failure; operation skipped but system healthy | `store.rs:272` — failed to parse proxy from Redis; `fetcher/proxyscrape.rs:35-37` — HTTP client build failed; `scheduler.rs:109` — failed to store a proxy |
| `info!` | Normal operational milestone; useful for monitoring | `scheduler.rs:93` — fetched N unique proxies; `scheduler.rs:167-173` — fetch cycle summary; `circuit.rs:55` — circuit tripped; `circuit.rs:68` — circuit reset; `geoip.rs:29` — database loaded |
| `debug!` | Detailed diagnostic; useful during development | `validator.rs:57` — individual proxy validation failure |

---

## Message Format

### Inline format strings with `{e}` for errors

```rust
// config.rs:273-276
tracing::warn!("cannot read config file {}: {e}, using defaults", path.display());
```

The `{e}` syntax captures the error as a tracing field. Prefer this over
`format!`-style string interpolation.

### Include identifying keys in messages

```rust
// store.rs:207
tracing::info!("circuit tripped for {}", updated.key());

// scheduler.rs:109
tracing::warn!("failed to store proxy {}: {e}", p.key());
```

Always include the proxy's `key()` (host:port) or `dedup_key()` (protocol:host:port)
so logs can be correlated to specific proxies.

### Fetcher messages prefix with fetcher name

```rust
// fetcher/proxyscrape.rs:35
tracing::warn!("{}: build client failed: {e}", self.name());
```

All fetcher implementations use `self.name()` as a prefix. This makes it easy
to filter logs by source.

---

## What to Log

| Event | Level | Required fields |
|-------|-------|-----------------|
| Service/component startup | `info!` | Component name, config values |
| Periodic cycle completion | `info!` | Counts (fetched, validated, stored, errors) |
| Circuit breaker state change | `info!` | Proxy key, new state, timestamp |
| External service failure (Redis, HTTP) | `warn!` | Operation, error detail |
| Individual proxy validation failure | `debug!` | Proxy key, error |
| Config file missing/invalid | `warn!` / `error!` | File path, error, fallback action |
| GeoIP database load failure | `error!` | Database path, error |
| Proxy parse failure from Redis | `warn!` | Error detail |

---

## What NOT to Log

| Data | Why | Handling |
|------|-----|----------|
| Full proxy URLs with credentials | Security risk | Log `key()` (host:port) only, never passwords |
| Redis connection URLs with passwords | Security risk | `config.rs` logs the file path, not the URL content |
| Full HTTP response bodies | Volume + potential PII | Log status code and error only |
| GeoIP lookup results for every proxy | Too verbose | Cache hits are silent; only log database load |
| GitHub API tokens | Secret | `GitHubDiscoverConfig.token` is `Option<String>` — never log it |

---

## Structured Fields

The codebase currently uses inline format strings rather than structured key-value
fields. When adding new log statements, prefer structured fields for machine-parseable
output:

```rust
// Current style (acceptable):
tracing::info!("fetched proxies ({} unique after dedup)", unique.len());

// Preferred style for new code:
tracing::info!(unique_count = unique.len(), "fetched proxies after dedup");
```

This allows log aggregators to filter on `unique_count` as a numeric field rather
than parsing it from the message string.

---

## Anti-Patterns

| Pattern | Why | Fix |
|---------|-----|-----|
| `println!` / `eprintln!` for diagnostics | Not captured by tracing subscriber | Use `tracing::debug!` or `tracing::info!` |
| `log::info!` | Wrong crate | Use `tracing::info!` |
| Logging inside tight loops without rate limiting | Log spam | Use `tracing::debug!` or add a counter + periodic summary |
| Logging full `Proxy` struct as debug | Verbose, may contain sensitive data | Log `proxy.key()` only |
| Empty error context: `tracing::warn!("error")` | No actionable information | Include the operation and error: `tracing::warn!("redis zadd failed: {e}")` |
