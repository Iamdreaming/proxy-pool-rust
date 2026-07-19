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
- Pool validation target URLs: `PoolSettings::effective_validate_target_urls()` falls back to `validate_target_url` when `validate_target_urls` is empty.
- Structured pool validation targets: `PoolSettings::effective_validate_targets()` prefers `validate_targets`, then `validate_target_urls`, then `validate_target_url`.
- Structured validation: `Validator::check_one(&self, proxy: &Proxy) -> ProxyCheckResult`.
- Compatibility validation: `Validator::validate_one(&self, proxy: &Proxy) -> Option<Proxy>` delegates to `check_one()`.
- Scheduler admission validation: `Validator::validate_many_against_targets(&self, proxies, targets, concurrency)` is strict all-target admission.
- Multi-target validation: `check_proxy_matrix(request: ProxyCheckMatrixRequest) -> Result<ProxyCheckMatrixResult, ProxyCheckMatrixError>`.

### 3. Contracts

Fetcher ids are stable machine ids used by API/MCP clients. Protocol-specific fetchers include the protocol, such as `proxyscrape:http` or `thespeedx:socks5`; single-source fetchers use stable snake-case ids such as `geonode`.

`FetcherRunReport` fields:

| Field | Type | Meaning |
|-------|------|---------|
| `id` | string | Stable fetcher id |
| `name` | string | Human-readable display name |
| `status` | enum | `never_run`, `success`, `empty`, `error`, `skipped` |
| `fetched` | integer | Raw candidate count when known |
| `parsed` | integer | Parsed proxy count |
| `unique` | integer | Deduplicated candidate count credited to this fetcher id |
| `validated` | integer | Deduplicated candidates from this fetcher that passed admission validation |
| `stored` | integer | Validated candidates from this fetcher that were stored successfully |
| `validation_survival_rate` | optional number | `validated / unique`, omitted when `unique` is zero |
| `error` | optional string | Error reason for failed fetch attempts |
| `circuit_state` | enum | Source circuit state: `closed`, `open`, or `half_open` |
| `consecutive_failures` | integer | Consecutive unsuccessful fetch attempts used by the source circuit |
| `last_error` | optional string | Latest failure detail retained across skips |
| `last_attempt_at` | optional RFC3339 datetime | Last real fetch attempt; automatic skips do not update this |
| `last_success_at` | optional RFC3339 datetime | Last successful source fetch |
| `opened_at` | optional RFC3339 datetime | When the source circuit entered open state |
| `next_probe_at` | optional RFC3339 datetime | Earliest automatic half-open probe time |
| `action` | optional enum | `fetched`, `skipped_open`, `half_open_probe`, or `manual_probe` |
| `started_at` / `finished_at` | optional RFC3339 datetime | Run timing |
| `duration_ms` | optional integer | Wall-clock run duration |

`ProxyCheckResult` fields:

| Field | Type | Meaning |
|-------|------|---------|
| `alive` | boolean | Whether the proxy validated successfully |
| `host` / `port` / `protocol` | proxy identity | Echoed from the checked proxy |
| `target_url` | string | Validation target URL used for the check |
| `target_host` | optional string | Parsed host from `target_url` |
| `latency_ms` | optional number | Present on success |
| `anonymity` | optional enum | Present on success |
| `http_status` | optional integer | Response status when headers were received |
| `timings` | optional object | `request_ms`, `body_read_ms`, and `total_ms` when available |
| `observed_ip` | optional string | Exit IP parsed from Cloudflare trace `ip=` or httpbin JSON `origin` |
| `observed_country` | optional string | Country/location code parsed from Cloudflare trace `loc=` |
| `error_type` | optional enum | Present on failure |
| `error` | optional string | Human-readable failure detail |

`ProxyCheckMatrixResult` fields:

| Field | Type | Meaning |
|-------|------|---------|
| `host` / `port` / `protocol` | proxy identity | Echoed from the checked proxy after validation |
| `target_count` | integer | Number of validation targets checked |
| `alive_count` | integer | Number of target checks that succeeded |
| `failed_count` | integer | Number of target checks that failed |
| `checks` | array | One `ProxyCheckResult` per target, preserving target URL diagnostics |

### 4. Validation & Error Matrix

| Condition | Contract |
|-----------|----------|
| Fetcher has never run | `status=never_run`, counts are zero |
| Fetcher succeeds with proxies | `status=success`, `parsed > 0` |
| Fetcher succeeds but parses no proxies | `status=empty`, no error |
| Fetcher build/fetch/body/parse fails | `status=error`, `error` contains the reason |
| Consecutive unsuccessful attempts reach source threshold | `circuit_state=open`, `next_probe_at` is set |
| Automatic refresh hits open source before `next_probe_at` | `status=skipped`, `action=skipped_open`, failure count unchanged |
| Automatic refresh reaches expired open source | Run as `action=half_open_probe` |
| Manual single-fetcher refresh hits open source | Run as `action=manual_probe`, even before `next_probe_at` |
| Probe success | `circuit_state=closed`, failure count reset |
| Probe failure | `circuit_state=open`, cooldown extended |
| Unknown fetcher id | `refresh_fetcher` returns `Err("fetcher not found: ...")` |
| Invalid proxy URL | `error_type=invalid_proxy_url` |
| Client construction fails | `error_type=client_build_failed` |
| Request timeout | `error_type=timeout`, `timings.request_ms` and `timings.total_ms` present |
| Other request failure | `error_type=request_failed`, request/total timings present |
| HTTP status >= 400 | `error_type=bad_status`, `http_status` and request/total timings present |
| Response body read fails | `error_type=body_read_failed`, `http_status` and phase timings present |
| Cloudflare trace body exposes `ip=` / `loc=` | `observed_ip` / `observed_country` populated |
| httpbin JSON body exposes `origin` | `observed_ip` populated from the first origin value |
| Matrix request omits targets | Defaults to Cloudflare trace and httpbin IP |
| Matrix request has blank host, zero port, invalid protocol, invalid target URL, or invalid timeout | Return deterministic request error before network calls |
| Matrix target network failure | Return a failed `ProxyCheckResult` entry, not a request-level error |

### 5. Good/Base/Bad Cases

- Good: `GET /api/fetchers` and MCP `fetcher_status` return the same `FetcherRunReport` shape from `SchedulerHandle`, including source circuit fields.
- Good: REST `/api/proxy/check-matrix` and MCP `check_proxy_matrix` serialize `ProxyCheckMatrixResult` directly from `proxy-core`.
- Base: a new legacy fetcher only implements `fetch()`; the default `fetch_with_report()` still returns a valid report with fetched/parsed counts equal to the returned proxy count.
- Bad: an API/MCP adapter parses logs or recomputes fetcher status locally. That duplicates business logic and will drift from scheduler state.

### 6. Tests Required

- `proxy-core` unit tests for report status constructors, source circuit transitions, validation result serialization, validation matrix request validation, and observed exit metadata parsing.
- `proxy-core` scheduler tests for refresh command compatibility and automatic-vs-manual source skip decisions.
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
- Attempt feedback: `UpstreamSelector::record_upstream_attempt(&self, upstream, status)`.
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
| `matched_reason` | string | `route_rule`, `route_default_group`, `direct_reachable_domain`, `business_domain_overseas`, `geoip_domestic`, `geoip_overseas`, `geoip_unknown_overseas`, or `general_fallback` |
| `geoip` | optional object | Country and overseas decision when GeoIP was consulted |
| `candidates` | array | Ordered exit candidates with availability and reason |
| `selected` | enum | First available exit: `direct`, `free_pool`, `warp`, `xray`, `no_proxy` |
| `unavailable` | array | Unavailable exits and skip reasons |

`candidates` is an ordered concrete-attempt list, not strictly a unique-exit
list. The same `exit` may appear more than once when an exit expands to several
runtime candidates. In particular, `free_pool` should expand to a small bounded
set of distinct weighted-random proxy candidates so a single bad pool proxy does
not terminate fallback for that exit.

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
| Router default group selected | `matched_reason=route_default_group`, `matched_rule=default` (no GeoIP, or default group is direct-only) |
| Router default group selected but host is a built-in direct-reachable domain | `matched_reason=direct_reachable_domain`, candidate order is `direct` |
| Router default group is direct but host is a built-in business overseas domain | `matched_reason=business_domain_overseas`, candidate order is premium-like (`xray -> warp -> no_proxy`) |
| Router default (non direct-only) + GeoIP domestic | `matched_reason=geoip_domestic`, candidate order is `direct`; `geoip` present |
| Router default (non direct-only) + GeoIP overseas / UNKNOWN | `matched_reason=geoip_overseas` or `geoip_unknown_overseas`; exits/tier from default group (example: premium) |
| Explicit non-default route rule matches a built-in business overseas domain | The explicit rule wins when its group maps to a known exit |
| No router but GeoIP available and domestic | Candidate order is `direct` |
| No router but GeoIP available and overseas | Candidate order is premium-like (`xray -> warp -> no_proxy`) |
| No router but GeoIP country is `UNKNOWN` | `matched_reason=geoip_unknown_overseas`, candidate order is premium-like |
| No router and no GeoIP | Candidate order is `free_pool -> warp -> xray -> no_proxy` |
| Free pool has several usable proxies | The decision may include repeated `exit=free_pool` candidates with different `detail` values |
| Gateway upstream connection fails before success response | Record `status=failure`, try later concrete candidates, and only then return HTTP 502 / SOCKS failure |
| Concrete `Upstream::Warp` fails in a gateway attempt | Call attempt feedback and put that WARP instance into the balancer's short business-failure cooldown |
| Concrete `Upstream::Proxy` fails in a gateway attempt | Call attempt feedback and put that proxy dedup key into a short process-local cooldown without writing Redis |
| Concrete `Upstream::Proxy` succeeds in a gateway attempt | Clear any process-local cooldown for that proxy dedup key |
| No concrete upstream exists | Record `exit=no_proxy,status=unavailable` |

### 5. Good/Base/Bad Cases

- Good: API `/api/routes/test`, MCP `route_test`, and gateway handlers all call the same `UpstreamSelector` instance built in `proxy-server`.
- Base: route dry-run returns candidate types and skip reasons without opening a target tunnel.
- Bad: API or MCP reconstructs routing from config, GeoIP, store, or logs locally. That duplicates selector behavior and will drift from gateway runtime decisions.

### 6. Tests Required

- `proxy-core` tests for `RouteDecision` serialization, route suffix diagnostics, candidate order helpers, and gateway metric rendering.
- `proxy-core` tests for weighted random multi-candidate pool selection without replacement.
- `proxy-core` tests for WARP balancer failure marking removing failed instances from healthy rotation.
- `proxy-core` tests for pool proxy failure cooldown active/expired/missing cases.
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
| `trend` | object | Recent-quality summary derived from bounded proxy history |
| `retention` | enum | `keep`, `below_min_score`, or `hard_failure_evict` |

`trend` fields:

| Field | Type | Meaning |
|-------|------|---------|
| `recent_samples` | integer | Number of retained validation observations |
| `recent_success_rate` | optional number | Successes divided by retained samples |
| `recent_latency_p50` | optional number | Median latency from retained successful observations |
| `recent_failures` | integer | Failed observations in the retained window |
| `last_checked_at_unix_secs` | optional integer | Unix timestamp for the newest retained sample |

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
| Stored proxy JSON has no quality history | Deserializes with an empty default trend |
| Score explanation has no retained samples | `trend.recent_samples=0`, optional trend fields are `null` |

### 5. Good/Base/Bad Cases

- Good: API and MCP call `ProxyStore::query_scored` and serialize `ScoredProxy`.
- Good: trend fields are derived by `proxy-core` from `Proxy.quality_history`.
- Base: `score(proxy, weights)` remains available and numerically compatible for Redis sorted-set ordering.
- Bad: API/MCP recompute latency, success, anonymity, trend, or retention locally. That creates drift from Redis score ordering and cleanup behavior.

### 6. Tests Required

- `proxy-core` tests for neutral score, fast elite score, component contributions, quality trend, below-min retention, hard-failure retention, and retention serialization.
- `proxy-api` response serialization test for `ScoredProxiesResponse`, including trend fields.
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

## Scenario: Pool Quality Status And Metrics

### 1. Scope / Trigger

- Trigger: shared `/api/status`, MCP `service_status`, and `/api/metrics`
  expose aggregate proxy quality state.
- The quality aggregation contract is owned by `proxy-core::status`; API and MCP
  must serialize `ServiceStatus` instead of recomputing aggregate quality fields.

### 2. Signatures

- Status collection:
  `collect_service_status(store, balancer, version, git_hash, uptime_sec, xray) -> ServiceStatus`.
- Quality status field: `ServiceStatus.quality: QualityStatus`.
- Metrics rendering: `render_prometheus_metrics(status: &ServiceStatus) -> String`.
- API endpoint: `GET /api/status` and `GET /api/metrics`.
- MCP tool: `service_status`.

### 3. Contracts

`QualityStatus` fields:

| Field | Type | Meaning |
|-------|------|---------|
| `total` | integer | Stored proxies scanned for quality aggregation |
| `score_buckets` | object | Counts for `untested`, `poor`, `fair`, `good`, `excellent` |
| `recent_samples` | integer | Total retained validation observations across the pool |
| `recent_success_rate` | optional number | Recent successes divided by recent samples; `null` when no samples exist |
| `recent_failures` | integer | Total retained failed observations |
| `stale_proxies` | integer | Proxies with no check or older than `stale_after_secs` |
| `stale_after_secs` | integer | Stale threshold used by the aggregation |
| `retention` | object | Counts for `below_min_score` and `hard_failure_evict` candidates |
| `top_failure_reasons` | array | Normalized recent failure reason counts |

Prometheus labels must stay bounded:

| Metric | Allowed labels |
|--------|----------------|
| `proxy_quality_score_bucket` | `bucket=untested|poor|fair|good|excellent` |
| `proxy_quality_retention_candidates` | `decision=below_min_score|hard_failure_evict` |
| `proxy_quality_failure_reasons_total` | normalized `reason`, never raw error text |

### 4. Validation & Error Matrix

| Condition | Contract |
|-----------|----------|
| Empty proxy pool | `quality.total=0`, all bucket/count fields are `0`, `recent_success_rate=null` |
| Proxy has no `last_check` and no trend samples | Counts as `untested` and stale |
| Proxy score is `<0.3`, `<0.6`, `<0.8`, or `>=0.8` | Counts as `poor`, `fair`, `good`, or `excellent` respectively |
| Recent sample has a failed observation | Included in `recent_failures` and normalized failure reason counts |
| Failure text contains URLs, addresses, or arbitrary details | Prometheus label uses bounded reason such as `timeout`, `request_failed`, or `other` |
| Redis quality scan fails | Status still serializes with default `quality`; `redis.status=error` carries the failure |

### 5. Good/Base/Bad Cases

- Good: REST `/api/status` and MCP `service_status` both expose the same
  `quality` object from `ServiceStatus`.
- Good: `/api/metrics` renders quality metrics from the status snapshot and does
  not read Redis or inspect proxies itself.
- Base: old stored proxy JSON without `quality_history` produces deterministic
  no-sample quality output.
- Bad: API, MCP, or frontend code recomputes bucket thresholds or failure reason
  labels locally.
- Bad: Prometheus labels contain proxy hosts, ports, full URLs, subscription
  values, or raw free-form errors.

### 6. Tests Required

- `proxy-core` tests for empty-pool output, bucket classification, stale
  classification, retention counts, failure reason normalization, and metrics
  rendering.
- REST integration smoke should assert `/api/status.quality` shape and
  `/api/metrics` quality metric names.
- MCP integration smoke should assert `service_status.quality` shape.

### 7. Wrong vs Correct

#### Wrong

```rust
// API layer invents buckets and labels independently.
let bucket = if proxy.latency_ms.unwrap_or(9999.0) < 300.0 { "fast" } else { "slow" };
```

#### Correct

```rust
let status = collect_service_status(&store, balancer, version, git_hash, uptime, xray).await;
let metrics = render_prometheus_metrics(&status);
```

The shared core status contract owns aggregate quality semantics; adapters only
serialize or render that contract.

## Scenario: Prometheus Low-Cardinality Contract

### 1. Scope / Trigger

- Trigger: `GET /api/metrics` exposition used by Prometheus scrapers and no-SSH
  operators.
- Owner: `proxy-core` rendering helpers. Adapters only concatenate the two
  render outputs; they must not invent labels.
- Goal: keep every label value on a compile-time fixed set or a normalized
  bounded set so series cardinality cannot grow with hosts, proxies, or raw
  errors.

### 2. Render Entry Points

| Source | Function | Assembler |
|--------|----------|-----------|
| Pool / quality / dependency | `proxy_core::status::render_prometheus_metrics` | `proxy-api` `routes.rs` `metrics` handler |
| Gateway route attempts | `GatewayRouteMetrics::render_prometheus` via `UpstreamSelector::render_gateway_metrics` | Same handler, appended after the status block |

`GET /api/metrics` currently concatenates **only** these two segments. There is
no third render source.

### 3. Full Metric Inventory

#### Unlabeled scalar gauges

| Metric | Meaning |
|--------|---------|
| `proxy_pool_tier` | 0–3 overseas exit reliability tier |
| `proxy_quality_recent_samples_total` | Recent validation samples retained |
| `proxy_quality_recent_success_rate` | Recent success rate (`0.0` when no samples) |
| `proxy_quality_recent_failures_total` | Recent failures retained |
| `proxy_quality_stale_proxies_total` | Proxies past the stale threshold |
| `proxy_quality_stale_after_seconds` | Stale threshold used for classification |
| `proxy_redis_ready` | Redis readiness `0`/`1` |
| `proxy_warp_instances_configured` | Configured WARP instances |
| `proxy_warp_instances_healthy` | Healthy WARP instances |
| `proxy_xray_active_nodes` | Active xray nodes |
| `proxy_xray_failed_nodes` | Failed xray nodes |
| `proxy_uptime_seconds` | Process uptime |

#### Finite / bounded labels

| Metric | Label | Allowed values |
|--------|-------|----------------|
| `proxy_pool_size` | `protocol` | `http`, `https`, `socks5`, `total` |
| `proxy_quality_score_bucket` | `bucket` | `untested`, `poor`, `fair`, `good`, `excellent` |
| `proxy_quality_retention_candidates` | `decision` | `below_min_score`, `hard_failure_evict` |
| `proxy_quality_failure_reasons_total` | `reason` | Normalized set below; top-N truncated to `MAX_FAILURE_REASON_METRICS = 5` |
| `proxy_gateway_route_attempts_total` | `protocol` | `http_connect`, `socks5`, `other` |
| same | `exit` | `direct`, `free_pool`, `warp`, `xray`, `no_proxy` |
| same | `status` | `success`, `failure`, `unavailable` |

Gateway counters always expand the full Cartesian product
`3 × 5 × 3 = 45` series (`METRIC_CELL_COUNT`), including zero-valued cells.
Series count does **not** grow with request host or proxy address.

#### Failure reason normalize set

`normalize_failure_reason` maps free-form error text to one of:

`unknown` | `timeout` | `bad_status` | `body_read_failed` | `invalid_proxy_url` |
`client_build_failed` | `request_failed` | `circuit_open` | `validation_failed` | `other`

### 4. Forbidden High-Cardinality Fields

Never use any of the following as a Prometheus **label value** (or as a
pseudo-label glued into a metric name):

- Proxy address / `host:port` / dedup key
- Full URL, subscription body, container dynamic ID
- Raw error strings or un-normalized free-form text
- Request target host (gateway must not label by destination domain)
- `git_hash`, image digest, or any unbounded identifier

`ServiceStatus.release` / `git_hash` remain available on status/MCP surfaces but
are **not** rendered as Prometheus metrics.

### 5. Not Present Today (Future Constraint)

| Area | Current state | Future rule |
|------|---------------|-------------|
| Fetcher | No `proxy_fetcher_*` metrics | If added, labels must obey this contract; never use fetcher free-form error text as a label |
| Release | No `proxy_release_*` metrics | If added, never use `git_hash` or image digest as a label |

### 6. Tests Required

- `metrics_label_allowlist_is_closed` — every `(metric, key, value)` from
  `render_prometheus_metrics` is in the allowlist; unlabeled names are locked.
- `metrics_failure_reason_render_has_no_high_cardinality_substrings` — raw URL /
  `host:port` errors normalize first; rendered label values never contain those
  substrings.
- `failure_reason_normalization_is_bounded` — covers all normalize branches.
- `gateway_metrics_emit_exactly_45_series` — fixed series count.
- `gateway_metrics_label_allowlist_is_closed` — protocol/exit/status values only.

### 7. Wrong vs Correct

#### Wrong

```rust
// High-cardinality: destination host and raw error as labels.
writeln!(
    out,
    "proxy_route{{host=\"{host}\",error=\"{raw_error}\"}} 1"
).ok();
```

#### Correct

```rust
// Bounded labels only; free-form text is normalized before rendering.
let reason = normalize_failure_reason(Some(raw_error));
writeln!(
    out,
    "proxy_quality_failure_reasons_total{{reason=\"{reason}\"}} {count}"
).ok();
// Gateway expands fixed protocol × exit × status cells (45 series).
```

Adapters must call the shared render helpers and never recompute label sets
locally.
