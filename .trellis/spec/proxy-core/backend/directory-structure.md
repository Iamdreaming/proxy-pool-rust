# Directory Structure

Module layout for `crates/proxy-core`.

---

## Top-Level Layout

```
crates/proxy-core/
├── Cargo.toml
└── src/
    ├── lib.rs              # Crate root — pub mod declarations only
    ├── models.rs           # Protocol, Anonymity, Proxy, WarpEndpoint, WarpInstance, EncryptedProxyState
    ├── config.rs           # Settings + sub-configs, load_settings(), serde default functions
    ├── store.rs            # ProxyStore (Redis ZSet), score(), weighted_random_choice()
    ├── scheduler.rs        # Scheduler (fetch→dedup→validate→store pipeline), SchedulerCommand, SchedulerHandle
    ├── validator.rs        # Validator (concurrent validation, anonymity detection)
    ├── circuit.rs          # Circuit breaker (Closed→Open→Half-Open), pure functions
    ├── geoip.rs            # GeoIPLookup (MaxMind GeoLite2 + Redis cache)
    ├── router.rs           # Router (longest-suffix domain match → exit group)
    ├── dedup.rs            # dedup() — HashSet by (protocol, host, port)
    ├── ewma.rs             # update_ewma() — exponentially weighted moving average
    ├── pacing.rs           # ConnectionPacer — rate-limits outbound connection attempts
    ├── xray_status.rs      # Shared xray lifecycle status registry and snapshots
    ├── fetcher.rs          # Fetcher trait re-export + build_fetchers() factory
    │   └── fetcher/
    │       ├── base.rs         # Fetcher trait (async_trait)
    │       ├── proxyscrape.rs  # ProxyScrape v2/v4 API
    │       ├── thespeedx.rs    # TheSpeedX GitHub raw lists
    │       ├── free_proxy_list.rs  # free-proxy-list.net HTML scraper
    │       ├── clarketm.rs     # clarketm GitHub raw list
    │       └── geonode.rs      # GeoNode JSON API
    └── warp.rs             # WARP sub-module root
        └── warp/
            ├── balancer.rs     # WarpBalancer (round-robin over healthy instances)
            └── health.rs       # WarpHealthChecker (periodic SOCKS5 probe)
```

---

## Module Roles

| Module | Responsibility | Key Public Types |
|--------|---------------|------------------|
| `models` | All domain data types | `Protocol`, `Anonymity`, `Proxy`, `WarpEndpoint`, `WarpInstance`, `EncryptedProxyState` |
| `config` | YAML configuration with serde defaults | `Settings`, `PoolSettings`, `WarpSettings`, `GeoIpSettings`, `XraySettings`, `load_settings()` |
| `store` | Redis-backed proxy storage | `ProxyStore`, `score()`, `weighted_random_choice()` |
| `scheduler` | Periodic fetch/validate pipeline | `Scheduler`, `SchedulerResult`, `SchedulerCommand`, `SchedulerHandle` |
| `validator` | Concurrent proxy validation | `Validator` |
| `circuit` | Circuit breaker state machine | `CircuitBreakerConfig`, `is_circuit_open()`, `trip()`, `reset()`, `should_trip()` |
| `geoip` | GeoIP lookup with caching | `GeoIPLookup`, `GeoInfo` |
| `router` | Domain-based routing + quality tiers | `Router`, `QualityTier`, `RouteMatch` |
| `route_debug` | Gateway route plan / selection / diagnostics | `UpstreamSelector`, `RouteDecision`, `RouteExit`, `exits_for_tier` |
| `dedup` | Proxy list deduplication | `dedup()` |
| `ewma` | Latency smoothing | `update_ewma()`, `DEFAULT_ALPHA` |
| `pacing` | Connection rate limiting | `ConnectionPacer` |
| `xray_status` | Shared lifecycle status for xray encrypted nodes | `XrayStatusRegistry`, `XrayStatusSnapshot`, `XrayNodeStatus` |
| `fetcher` | Source fetcher trait + impls | `Fetcher` trait, `build_fetchers()` |
| `warp` | WARP instance management | `WarpBalancer`, `WarpHealthChecker` |

---

## Sub-Module Pattern

Two sub-modules use the `mod_name.rs` + `mod_name/` directory pattern (not `mod.rs`):

- **fetcher**: `fetcher.rs` declares `pub mod base; pub mod proxyscrape; ...` and re-exports `pub use base::Fetcher;`. The factory function `build_fetchers()` lives in `fetcher.rs`.
- **warp**: `warp.rs` declares `pub mod balancer; pub mod health;`. No re-exports.

When adding a new fetcher: create `src/fetcher/new_source.rs`, add `pub mod new_source;` in `fetcher.rs`, and wire it into `build_fetchers()` with a `FetcherToggle` field in `FetchersConfig`.

For multiple public raw-list sources that only differ by URL, parser kind, and
fallback protocol, extend `fetcher/public_lists.rs` instead of copying a full
HTTP fetcher module. Keep ids stable (`source:variant`) because
`refresh_fetcher`, fetcher status, source circuits, and validation survival
metrics all key on fetcher id.

---

## Naming Conventions

- **Files**: `snake_case`, matching the module name (`free_proxy_list.rs` for `free_proxy_list` mod).
- **Structs/Enums**: `PascalCase` (`ProxyStore`, `CircuitBreakerConfig`).
- **Functions/Methods**: `snake_case` (`load_settings`, `mark_failed_with_circuit`).
- **Constants**: `SCREAMING_SNAKE_CASE` for module-level constants (`DEFAULT_ALPHA`, `HTTP_URL`); `snake_case` for serde default functions (`default_fetch_interval`).
- **Redis keys**: `snake_case` with colon separator (`proxies:http`, `geoip_cache:1.2.3.4`).

---

## Examples of Well-Organised Modules

- **circuit.rs** — Pure functions only, no struct methods. Easy to test, no state mutation. All functions take `&Proxy` + config and return a new `Proxy`.
- **dedup.rs** — Single public function, inline `#[cfg(test)]` module. Minimal surface area.
- **ewma.rs** — One pure function + one constant. Self-documenting.
