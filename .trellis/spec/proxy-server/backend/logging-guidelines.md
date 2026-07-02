# Logging Guidelines

How `tracing` is used in `proxy-server` startup and service lifecycle.

---

## Logging Setup

```rust
// main.rs:38-45
fn setup_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}
```

- Default level: `info` (overridden by `RUST_LOG` env var).
- Format: default `tracing_subscriber::fmt` (compact, with timestamps).
- No structured/JSON logging configured — add `.json()` to the builder if needed.

---

## Log Level Conventions

### During Startup

| Level | When | Example |
|-------|------|---------|
| `info!` | Each component successfully initialized | `"loaded configuration from {config_path}"`, `"connected to Redis at {url}"`, `"proxy gateway listening on {addr}"` |
| `warn!` | Non-critical failure that degrades functionality | `"xray gRPC initial connect failed: {e} (reconnect loop will retry)"` |
| `error!` | Startup failure that skips a feature entirely | `"failed to load routing rules from {path}: {e}"`, `"xray: failed to start process: {e}"` |

### During Runtime

| Level | When | Example |
|-------|------|---------|
| `info!` | Periodic cycle completion | `"fetch cycle: fetched=X, validated=Y, stored=Z, errors=E"` |
| `info!` | State transitions | `"circuit tripped for {key}"`, `"outbound_sync: activated {tag} -> local port {port}"` |
| `warn!` | Recoverable operation failure | `"outbound_sync: add_inbound failed: {e}"`, `"subscription source: fetch failed: {e}"` |
| `debug!` | Low-level per-connection diagnostics | `"connection error from {addr}: {e}"` |

---

## What to Log

### Always Log

- Service start/stop events with identifying info (address, port, mode).
- Configuration loaded (at least the config path).
- Redis/external service connections.
- Error conditions before fallback/degradation.
- Periodic cycle summaries (fetch, validate, sync).

### Never Log

- Full proxy lists (can be large; use count instead).
- Raw subscription content (may contain credentials).
- Per-request bodies in the gateway.
- xray config JSON (contains passwords) — log the tag and port only.

---

## Structured Fields

When logging in `main.rs`, prefer structured key-value pairs for machine parsing:

```rust
// Good — structured fields
tracing::info!(sleep_secs = interval.as_secs(), "subscription refresh cycle sleeping");

// Acceptable — simple message
tracing::info!("proxy gateway listening on {addr}");

// Avoid — string interpolation for structured data
// Do NOT use format!() inside tracing macros for data that should be structured
```

---

## Log Messages for Service Lifecycle

Each service should log at `info!` level when it starts and stops:

```
INFO proxy_pool: loaded configuration from config/settings.yaml
INFO proxy_pool: connected to Redis at redis://localhost:6379/0
INFO proxy_pool: proxy gateway listening on 0.0.0.0:9080
INFO proxy_pool: API server listening on 0.0.0.0:8000
INFO proxy_pool: MCP server starting on stdio transport
INFO proxy_pool: xray-core process started
INFO proxy_pool: xray gRPC client connected to http://127.0.0.1:10085
INFO proxy_pool: outbound_sync: starting (interval=30s)
```

When a service stops, log the result:

```
INFO proxy_pool: scheduler stopped: Ok(())
INFO proxy_pool: health checker stopped: Err(JoinError::Panic(...))
```

---

## Common Mistakes

1. **Logging sensitive data**: xray outbound configs contain `password` fields.
   Log only `tag` and `local_socks5_port`, never the full JSON.

2. **Using `println!` instead of `tracing`**: All output must go through `tracing`
   so that `RUST_LOG` filtering works consistently.

3. **Missing startup log for conditional features**: When xray is disabled, log
   the fact: `"xray integration disabled (set xray.enabled=true to enable)"`.

4. **Over-logging in hot paths**: The gateway processes many connections per second.
   Use `debug!` for per-connection errors, `info!` only for the initial "listening" message.
