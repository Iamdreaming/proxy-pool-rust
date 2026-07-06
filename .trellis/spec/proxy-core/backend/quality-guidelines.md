# Quality Guidelines

Code standards, forbidden patterns, and Redis storage conventions for `proxy-core`.

---

## Lint Rules

```bash
cargo clippy -- -D warnings
```

Zero warnings required before commit. This is enforced in CI and by the project
CLAUDE.md.

---

## Forbidden Patterns

| Pattern | Why | Example | Fix |
|---------|-----|---------|-----|
| `unwrap()` / `expect()` in non-test code | Panics in production | `conn.zadd(...).await.unwrap()` | `conn.zadd(...).await?` with proper error propagation |
| `log` crate macros | Project uses `tracing` exclusively | `log::info!("...")` | `tracing::info!("...")` |
| `std::sync::Mutex` in async context | Blocks the tokio runtime | `std::sync::Mutex::new(state)` | `tokio::sync::Mutex` (see `pacing.rs:16`) |
| Blocking DNS in hot path | Stalls the executor | `std::net::ToSocketAddrs` in `geoip.rs:127` | Use `tokio::net::lookup_host` — current code is acceptable only because GeoIP is called once per proxy, not in a tight loop |
| `clone()` on large structs without justification | Unnecessary allocation | `proxies.clone()` | Pass by reference or use `Arc` |
| Mutable global state | Untestable, race-prone | `static mut X: ...` | Use `Arc<RwLock<...>>` or channel-based state |
| `serde_yaml::from_str("{}").unwrap()` in hand-written Default | Works but fragile | All sub-config Default impls in `config.rs` | Acceptable because `serde(default)` guarantees all fields have defaults; do not add new sub-configs without `#[serde(default)]` on every field |

---

## Required Patterns

### Every config field must have `#[serde(default)]`

```rust
// config.rs:97-117
pub struct PoolSettings {
    #[serde(default = "default_fetch_interval")]
    pub fetch_interval_sec: u64,
    #[serde(default = "default_validate_interval")]
    pub validate_interval_sec: u64,
    // ... every field has a default
}
```

A missing key in YAML must never cause a deserialization error. Primitive fields
use `#[serde(default = "function_name")]`; struct fields use `#[serde(default)]`
which delegates to the sub-config's `Default` impl.

### Sub-config Default impls delegate to serde

```rust
// config.rs:436-440
impl Default for GatewaySettings {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}
```

This pattern is consistent across all sub-configs. The `unwrap()` is safe here
because `serde(default)` on every field guarantees `{}` deserialises successfully.

### Redis operations use `Arc<MultiplexedConnection>`

```rust
// store.rs:61-66
pub struct ProxyStore {
    conn: Arc<MultiplexedConnection>,
    // ...
}
```

`MultiplexedConnection` is cheaply cloneable and shares the underlying connection
multiplexer. The `conn()` helper clones it to get a fresh handle for each
operation because `redis::AsyncCommands` requires `&mut self`.

### Circuit breaker is pure functions

```rust
// circuit.rs:49-61
pub fn trip(proxy: &Proxy, config: &CircuitBreakerConfig) -> Proxy {
    let mut updated = proxy.clone();
    updated.circuit_open = true;
    updated.circuit_open_until = Some(open_until);
    updated
}
```

Circuit breaker functions return a **new `Proxy`** rather than mutating in place.
This makes them easy to test and compose. The caller (`ProxyStore`) is responsible
for persisting the updated proxy.

### Fetcher trait returns `Vec<Proxy>`, never `Result`

```rust
// fetcher/base.rs:7-8
async fn fetch(&self) -> Vec<Proxy>;
```

Fetchers are best-effort sources. A failed HTTP request returns an empty vec with
a `tracing::warn!` log. The `Scheduler` aggregates results from all fetchers, so
one failing source does not block others.

### Bounded concurrency with semaphore

```rust
// validator.rs:80-81
let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
```

The validator always uses a semaphore to cap concurrent outbound connections.
The concurrency value comes from `PoolSettings.validate_concurrency` (default: 100).

---

## Redis Storage Conventions

### Key schema

| Key pattern | Type | Value | Score |
|-------------|------|-------|-------|
| `proxies:{protocol}` | ZSet | JSON-serialised `Proxy` | `score(proxy, weights)` in [0, 1] |
| `geoip_cache:{host}` | String | JSON-serialised `GeoInfo` | TTL: `cache_ttl_sec` |

### Upsert pattern (add / mark_success / mark_failed)

All write operations follow the same pattern:

1. **Remove existing**: `remove_existing()` scans the ZSet for a member matching
   `host:port:protocol` and removes it. This is O(N) per protocol — acceptable
   because ZSets are bounded by `min_score` eviction.
2. **Mutate the `Proxy` struct**: increment counters, update circuit breaker, etc.
3. **Re-score and re-insert**: `zadd(key, member, score)`.

```rust
// store.rs:183-192
pub async fn mark_success(&self, proxy: &Proxy) -> anyhow::Result<()> {
    self.remove_existing(&proxy.protocol, proxy).await?;
    let mut updated = proxy.clone();
    updated.success_count += 1;
    let s = score(&updated, &self.weights);
    let member = serde_json::to_string(&updated)?;
    let mut conn = self.conn();
    let _: () = conn.zadd(redis_key(&updated.protocol), &member, s).await?;
    Ok(())
}
```

### Scoring formula

```rust
// store.rs:11-26
pub fn score(proxy: &Proxy, weights: &ScoreWeights) -> f64 {
    let latency_norm = ((2000.0 - latency) / 2000.0).clamp(0.0, 1.0);
    let success_rate = ((success - fail) / total).clamp(0.0, 1.0);
    let anonymity = proxy.anonymity.map(|a| a.bonus()).unwrap_or(0.0);
    weights.latency * latency_norm + weights.success * success_rate + weights.anonymity * anonymity
}
```

Default weights: latency=0.5, success=0.3, anonymity=0.2. Untested proxies
get a neutral success_rate of 0.5. The score is always in [0, 1].

### Eviction

Proxies are evicted when:
- Hard eviction: `fail_count > max(5, success_count * 2)` (store.rs:170)
- Score eviction: `score < min_score` (default 0.1)

Evicted proxies are simply not re-inserted after `remove_existing`.

---

## Testing Requirements

- Every new function must have at least a happy-path test.
- Tests live in `#[cfg(test)] mod tests` at the bottom of each source file.
- Integration tests go in `crates/proxy-core/tests/` (currently empty; Redis-dependent
  tests should use `redis_test` or mock connections).
- `cargo test` must pass with zero failures before commit.

Current test coverage:

| Module | Tests |
|--------|-------|
| `dedup` | `test_dedup` — verifies duplicates by (protocol, host, port) are removed |
| `router` | `test_router_match` — verifies longest-suffix matching and default fallback |
| `scheduler` | `test_scheduler_result_default`, `test_scheduler_result_serialize`, `test_scheduler_handle_refresh`, `test_scheduler_handle_closed_channel` |
| `circuit`, `ewma`, `pacing`, `store`, `validator`, `geoip` | No tests yet — priority for next sprint |

---

## Code Review Checklist

- [ ] All public items have `///` doc comments
- [ ] Every config field has `#[serde(default = "...")]` or `#[serde(default)]`
- [ ] No `unwrap()` / `expect()` outside `#[cfg(test)]`
- [ ] No `log` crate usage — `tracing` only
- [ ] Redis operations propagate errors via `?`, not silently swallowed
- [ ] New fetcher implementations follow the `fetch() -> Vec<Proxy>` pattern
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo test` passes

## Scenario: Fetcher Run Reports And Validation Check Results

### 1. Scope / Trigger

- Trigger: API and MCP expose fetcher status, single-fetcher refresh, and structured proxy check results.
- This is a cross-layer contract owned by `proxy-core`; adapters in `proxy-api` and `proxy-mcp` must serialize core types rather than reimplementing fetcher or validator logic.

### 2. Signatures

- Trait compatibility: `Fetcher::fetch(&self) -> Vec<Proxy>` remains available.
- Structured fetch: `Fetcher::fetch_with_report(&self) -> FetcherOutput`.
- Scheduler status: `SchedulerHandle::fetcher_statuses(&self) -> Vec<FetcherRunReport>`.
- Single refresh: `SchedulerHandle::refresh_fetcher(&self, fetcher_id) -> anyhow::Result<SchedulerResult>`.
- Structured validation: `Validator::check_one(&self, proxy: &Proxy) -> ProxyCheckResult`.
- Compatibility validation: `Validator::validate_one(&self, proxy: &Proxy) -> Option<Proxy>` delegates to `check_one()`.

### 3. Contracts

Fetcher ids are stable machine ids used by API/MCP clients. Protocol-specific fetchers include the protocol, such as `proxyscrape:http` or `thespeedx:socks5`; single-source fetchers use stable snake-case ids such as `geonode`.

`FetcherRunReport` fields:

| Field | Type | Meaning |
|-------|------|---------|
| `id` | string | Stable fetcher id |
| `name` | string | Human-readable display name |
| `status` | enum | `never_run`, `success`, `empty`, `error` |
| `fetched` | integer | Raw candidate count when known |
| `parsed` | integer | Parsed proxy count |
| `error` | optional string | Error reason for failed fetch attempts |
| `started_at` / `finished_at` | optional RFC3339 datetime | Run timing |
| `duration_ms` | optional integer | Wall-clock run duration |

`ProxyCheckResult` fields:

| Field | Type | Meaning |
|-------|------|---------|
| `alive` | boolean | Whether the proxy validated successfully |
| `host` / `port` / `protocol` | proxy identity | Echoed from the checked proxy |
| `latency_ms` | optional number | Present on success |
| `anonymity` | optional enum | Present on success |
| `error_type` | optional enum | Present on failure |
| `error` | optional string | Human-readable failure detail |

### 4. Validation & Error Matrix

| Condition | Contract |
|-----------|----------|
| Fetcher has never run | `status=never_run`, counts are zero |
| Fetcher succeeds with proxies | `status=success`, `parsed > 0` |
| Fetcher succeeds but parses no proxies | `status=empty`, no error |
| Fetcher build/fetch/body/parse fails | `status=error`, `error` contains the reason |
| Unknown fetcher id | `refresh_fetcher` returns `Err("fetcher not found: ...")` |
| Invalid proxy URL | `error_type=invalid_proxy_url` |
| Client construction fails | `error_type=client_build_failed` |
| Request timeout | `error_type=timeout` |
| Other request failure | `error_type=request_failed` |
| HTTP status >= 400 | `error_type=bad_status` |
| Response body read fails | `error_type=body_read_failed` |

### 5. Good/Base/Bad Cases

- Good: `GET /api/fetchers` and MCP `fetcher_status` return the same `FetcherRunReport` shape from `SchedulerHandle`.
- Base: a new legacy fetcher only implements `fetch()`; the default `fetch_with_report()` still returns a valid report with fetched/parsed counts equal to the returned proxy count.
- Bad: an API/MCP adapter parses logs or recomputes fetcher status locally. That duplicates business logic and will drift from scheduler state.

### 6. Tests Required

- `proxy-core` unit tests for report status constructors and validation result serialization.
- `proxy-core` scheduler tests for refresh command compatibility.
- `proxy-api` serialization tests for refresh and fetcher status response structs.
- `proxy-mcp` deserialization tests for new tool params.
- Deployed integration tests should assert `/api/fetchers`, MCP `fetcher_status`, and MCP tool listing include the new operations.

### 7. Wrong vs Correct

#### Wrong

```rust
// API layer invents status from logs or counts.
let fetchers = vec![json!({"name": "ProxyScrape", "status": "ok"})];
```

This makes API semantics diverge from the scheduler's actual latest run.

#### Correct

```rust
let fetchers = state.scheduler_handle.fetcher_statuses().await;
Json(FetchersResponse { fetchers })
```

The scheduler owns fetcher state; API and MCP only serialize it.
