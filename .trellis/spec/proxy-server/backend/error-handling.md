# Error Handling

How errors are handled during startup and service lifecycle in `proxy-server`.

---

## Startup Errors

`main()` returns `anyhow::Result<()>`. Critical startup failures propagate immediately:

| Failure | Behavior | Line |
|---------|----------|------|
| Primary Redis connection | **Fatal** via `?` from `main()` because the store is central to API, gateway, scheduler, and MCP | startup Redis connection |
| Subscription Redis connection | **Log + skip** — subscription refresh is disabled when its secondary connection cannot be opened | subscription setup |
| Config file missing | **Graceful fallback** — uses defaults, logs warning | `config.rs:267–269` |
| Config YAML invalid | **Graceful fallback** — uses defaults, logs error | `config.rs:282–288` |
| Xray process start | **Log + skip** — `xray_supervisor_handle = None`, service continues without xray | 200–217 |
| Xray gRPC initial connect | **Log warning** — reconnect loop will retry | 232–233 |
| Routes YAML invalid | **Log error** — `router = None`, falls back to default routing | 116–129 |
| GeoIP database missing | **Log + skip** — `geoip = None`, no GeoIP-based routing | 130–141 |

### Pattern: Non-critical = Graceful Degradation

Most startup failures are non-critical. The server starts with reduced functionality
rather than crashing. Only the Redis connection is treated as fatal (the store is
central to all operations).

```rust
// main.rs:130-141 — GeoIP: missing database is non-fatal
let geoip = if settings.geoip.database_path
    != proxy_core::config::GeoIpSettings::default().database_path
    || std::path::Path::new(&settings.geoip.database_path).exists()
{
    Some(Arc::new(Mutex::new(proxy_core::geoip::GeoIPLookup::new(
        redis_for_geoip,
        &settings.geoip,
    ))))
} else {
    tracing::info!("geoip: database not found, skipping GeoIP-based routing");
    None
};
```

---

## Service Lifecycle Errors

### Crashed Services

The `tokio::select!` block (lines 361–370) waits for **any** service to finish.
When a service task exits (whether by error or normal completion), the entire
process logs it and continues waiting for other services — there is **no automatic
restart** at the `proxy-server` level.

```rust
tokio::select! {
    r = scheduler_task => tracing::info!("scheduler stopped: {:?}", r),
    r = health_handle => tracing::info!("health checker stopped: {:?}", r),
    // ...
}
```

**Implication**: If the scheduler panics, the API/gateway/MCP continue running but
with a stale proxy pool. Individual services that need resilience must implement
their own restart logic (e.g., `XrayProcess::supervise` has built-in restart).

### Individual Service Error Handling

| Service | Error Strategy |
|---------|---------------|
| Scheduler fetch loop | `tracing::info!` the result, sleep, retry next interval |
| Scheduler validate loop | `tracing::error!` on failure, sleep, retry next interval |
| WarpHealthChecker | Individual probe failures logged inside `check_once()` |
| API (axum) | Logs bind/serve errors explicitly; the API task exits so the main `select!` reports a fatal core-service stop |
| Gateway (TCP) | `?` in `run()` propagates bind/accept errors |
| MCP | Errors from `serve()` mapped to `anyhow` and returned |
| XrayProcess | Built-in supervisor with exponential backoff restart |
| XrayClient | Built-in reconnect loop with exponential backoff |
| OutboundSync | Pauses when gRPC disconnected, resumes on reconnect |

---

## Forbidden Patterns

| Pattern | Why | Fix |
|---------|-----|-----|
| `unwrap()` on service bind/listen | Panics with no recovery | Use `?` to propagate, or `.expect("...")` with context |
| Silent service crash | Other services continue with stale state | Log at `error!` level and consider alerting |
| Blocking the main function | Prevents other services from starting | Use `tokio::spawn` for all long-running work |
| Creating Redis connections inside loops | Connection exhaustion | Create once in `main()`, share via `Arc` |

---

## Common Mistakes

1. **Forgetting to clone Arc before spawn**: `tokio::spawn` requires `'static + Send`.
   Clone `Arc` references before the spawn closure:
   ```rust
   let store = store.clone(); // Arc clone — cheap
   tokio::spawn(async move { store.all(Protocol::Http).await });
   ```

2. **Using `unwrap()` on TCP bind**: If the port is already in use, the process
   panics with an unhelpful message. Log the bind error with the address and
   let the service task exit so the main `select!` can report the fatal stop.

3. **Not handling the xray-disabled case**: Code that accesses xray handles must
   check for `None`. The `tokio::select!` uses `std::future::pending()` as a no-op
   placeholder when xray is disabled.

4. **Creating a new Redis connection per component**: Each `get_multiplexed_async_connection()`
   call opens a new connection. Reuse `redis_multiplexed.clone()` where possible
   (it's cheaply cloneable). Only the subscription and xray subsystems need separate
   connections due to their distinct lifecycle.
