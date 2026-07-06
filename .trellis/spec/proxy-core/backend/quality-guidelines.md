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
- Hard eviction: `fail_count > max(8, success_count * 3)` (store.rs)
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

## Scenario: Gateway Route Decisions And Fallback Diagnostics

### 1. Scope / Trigger

- Trigger: gateway route dry-run, MCP `route_test`, gateway fallback metrics, and runtime fallback tracing span `proxy-core`, `proxy-gateway`, `proxy-api`, `proxy-mcp`, and `proxy-server`.
- The route decision contract is owned by `proxy-core::route_debug`; adapters and protocol handlers must not reimplement route selection logic.

### 2. Signatures

- Route matching diagnostics: `Router::match_route(&self, host: &str) -> RouteMatch`.
- Runtime selection: `UpstreamSelector::select(&self, host, protocol) -> Upstream` remains available for compatibility.
- Traceable selection: `UpstreamSelector::select_with_trace(&self, host, protocol) -> RouteSelection`.
- Dry-run: `UpstreamSelector::dry_run(&self, host, protocol) -> RouteDecision`.
- Metrics render: `UpstreamSelector::render_gateway_metrics(&self) -> String`.
- API endpoint: `GET /api/routes/test?host=<host>&protocol=<protocol>`.
- MCP tool: `route_test` with `{ "host": "...", "protocol": "http|https|socks4|socks5" }`.

### 3. Contracts

`RouteDecision` fields:

| Field | Type | Meaning |
|-------|------|---------|
| `host` | string | Normalized target host evaluated by the selector |
| `protocol` | string | Protocol used for pool lookup; invalid input falls back to `http` |
| `matched_group` | optional string | Configured route group, when a router exists |
| `matched_rule` | optional string | Matched suffix rule or `default`, when a router exists |
| `matched_reason` | string | `route_rule`, `route_default_group`, `geoip_domestic`, `geoip_overseas`, or `general_fallback` |
| `geoip` | optional object | Country and overseas decision when GeoIP was consulted |
| `candidates` | array | Ordered exit candidates with availability and reason |
| `selected` | enum | First available exit: `direct`, `free_pool`, `warp`, `xray`, `no_proxy` |
| `unavailable` | array | Unavailable exits and skip reasons |

Gateway route metrics use:

```text
proxy_gateway_route_attempts_total{protocol="<http_connect|socks5|other>",exit="<direct|free_pool|warp|xray|no_proxy>",status="<success|failure|unavailable>"} <count>
```

### 4. Validation & Error Matrix

| Condition | Contract |
|-----------|----------|
| Missing or empty API `host` | HTTP 400 with structured JSON |
| Missing MCP `protocol` | Defaults to `http` through MCP protocol resolution |
| Unknown protocol | Falls back to `http`, matching existing MCP behavior |
| Router suffix matches | `matched_reason=route_rule`, `matched_rule=<suffix>` |
| Router default group selected | `matched_reason=route_default_group`, `matched_rule=default` |
| No router but GeoIP available and domestic | Candidate order is `direct` |
| No router but GeoIP available and overseas | Candidate order is `warp -> xray -> free_pool -> no_proxy` |
| No router and no GeoIP | Candidate order is `free_pool -> warp -> xray -> no_proxy` |
| Gateway upstream connection fails before success response | Record `status=failure`, try later concrete candidates, and only then return HTTP 502 / SOCKS failure |
| No concrete upstream exists | Record `exit=no_proxy,status=unavailable` |

### 5. Good/Base/Bad Cases

- Good: API `/api/routes/test`, MCP `route_test`, and gateway handlers all call the same `UpstreamSelector` instance built in `proxy-server`.
- Base: route dry-run returns candidate types and skip reasons without opening a target tunnel.
- Bad: API or MCP reconstructs routing from config, GeoIP, store, or logs locally. That duplicates selector behavior and will drift from gateway runtime decisions.

### 6. Tests Required

- `proxy-core` tests for `RouteDecision` serialization, route suffix diagnostics, candidate order helpers, and gateway metric rendering.
- `proxy-gateway` tests for upstream variants and connection helper compatibility.
- `proxy-api` tests for `RouteTestResponse` serialization and route query deserialization.
- `proxy-mcp` tests for `RouteTestParam` required and optional fields.
- Integration tests should assert `/api/routes/test`, `/api/metrics` gateway labels, MCP tool listing, and MCP `route_test` response shape.

### 7. Wrong vs Correct

#### Wrong

```rust
// API layer reimplements route behavior from route group strings.
let selected = if host.ends_with(".cn") { "direct" } else { "warp" };
```

This diverges as soon as the gateway fallback order, GeoIP behavior, or route groups change.

#### Correct

```rust
let decision = state.route_selector.dry_run(host, protocol).await;
Json(RouteTestResponse {
    status: "ok".into(),
    decision: Some(decision),
})
```

The selector owns route decisions; API, MCP, metrics, and gateway handlers consume that shared contract.

## Scenario: Score Explanation And Low-Score Cleanup

### 1. Scope / Trigger

- Trigger: proxy score explanations, REST `/api/proxies/scores`, MCP `explain_proxy_scores`, and MCP `cleanup_low_score_proxies` span `proxy-core`, `proxy-api`, and `proxy-mcp`.
- The score formula and retention decision contract are owned by `proxy-core::store`; adapters must call core store helpers instead of recomputing score components.

### 2. Signatures

- Compatibility score: `score(proxy: &Proxy, weights: &ScoreWeights) -> f64`.
- Explanation: `explain_score(proxy: &Proxy, weights: &ScoreWeights, min_score: f64) -> ScoreExplanation`.
- Store explanation: `ProxyStore::explain(&self, proxy: &Proxy) -> ScoreExplanation`.
- Query with scores: `ProxyStore::query_scored(protocol, filter, limit) -> anyhow::Result<Vec<ScoredProxy>>`.
- Cleanup: `ProxyStore::cleanup_low_score(protocol, limit, min_score, apply) -> anyhow::Result<CleanupLowScoreResult>`.
- API endpoint: `GET /api/proxies/scores`.
- MCP tools: `explain_proxy_scores`, `cleanup_low_score_proxies`.

### 3. Contracts

`ScoreExplanation` fields:

| Field | Type | Meaning |
|-------|------|---------|
| `score` | number | Final weighted score |
| `min_score` | number | Retention threshold used for this explanation |
| `latency` | object | Raw latency, normalized value, weight, and contribution |
| `success` | object | Success/fail counts, success rate, weight, and contribution |
| `anonymity` | object | Raw anonymity, normalized value, weight, and contribution |
| `retention` | enum | `keep`, `below_min_score`, or `hard_failure_evict` |

Cleanup is dry-run by default. MCP callers must pass `apply: true` before any stored proxy is removed.

### 4. Validation & Error Matrix

| Condition | Contract |
|-----------|----------|
| Unknown latency | Uses `5000ms`, normalized to `0.0` |
| No success/failure observations | Uses neutral success rate `0.5` |
| Unknown anonymity | Uses anonymity normalized value `0.0` |
| `score < min_score` | `retention=below_min_score` |
| `fail_count > max(8, success_count * 3)` | `retention=hard_failure_evict` |
| Hard failure and below min score both apply | Hard failure wins |
| `cleanup_low_score_proxies` called without `apply` | Returns candidates but removes zero proxies |
| Store query/remove fails | API returns 500; MCP returns `Err("Error: ...")` |

### 5. Good/Base/Bad Cases

- Good: API and MCP call `ProxyStore::query_scored` and serialize `ScoredProxy`.
- Base: `score(proxy, weights)` remains available and numerically compatible for Redis sorted-set ordering.
- Bad: API/MCP recompute latency, success, anonymity, or retention locally. That creates drift from Redis score ordering and cleanup behavior.

### 6. Tests Required

- `proxy-core` tests for neutral score, fast elite score, component contributions, below-min retention, hard-failure retention, and retention serialization.
- `proxy-api` response serialization test for `ScoredProxiesResponse`.
- `proxy-mcp` parameter deserialization tests for `CleanupLowScoreParam`.
- Integration tests should assert `/api/proxies/scores` response shape and MCP tool listing includes `explain_proxy_scores` and `cleanup_low_score_proxies`.

### 7. Wrong vs Correct

#### Wrong

```rust
// API layer recomputes score details.
let score = proxy.success_count as f64 / (proxy.success_count + proxy.fail_count) as f64;
```

#### Correct

```rust
let scored = state.store.query_scored(protocol, &filter, limit).await?;
Json(ScoredProxiesResponse {
    protocol: protocol_str.to_string(),
    count: scored.len(),
    proxies: scored,
})
```

The store owns score math and retention decisions; adapters only select inputs and serialize outputs.
