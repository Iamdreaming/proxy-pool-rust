# Directory Structure

Module layout for `crates/proxy-server`.

---

## Layout

```
crates/proxy-server/
├── Cargo.toml      # Depends on ALL other workspace crates
└── src/
    └── main.rs     # Single file (~374 lines): setup + service launch
```

This crate is intentionally a **single-file binary** — no `lib.rs`, no sub-modules.
All business logic lives in other crates; `main.rs` is pure wiring and startup.

---

## Startup Sequence

`main.rs` performs the following steps in order (lines referenced from `src/main.rs`):

| Step | Lines | What |
|------|-------|------|
| 1 | 38–45 | `setup_logging()` — `tracing_subscriber` with `EnvFilter` (default: `info`) |
| 2 | 52–55 | Load config from YAML (path via CLI arg, defaults to `config/settings.yaml`) |
| 3 | 59–61 | Connect to Redis (`get_multiplexed_async_connection`) |
| 4 | 66–72 | Build `ProxyStore` with `ScoreWeights`, `min_score`, `CircuitBreakerConfig` |
| 5 | 75–86 | Build WARP: `WarpInstance` list → `WarpBalancer` + `WarpHealthChecker` |
| 6 | 89–108 | Build fetchers (`build_fetchers`), `Validator` (optional `ConnectionPacer`), `Scheduler` |
| 7 | 110–112 | Create `SchedulerHandle` via `mpsc::channel` |
| 8 | 115–149 | Build `UpstreamSelector` with optional `Router` (from YAML) + `GeoIPLookup` |
| 9 | 152–159 | Build `AppState` + axum API router |
| 10 | 162–165 | Build `ProxyGateway` with `UpstreamSelector` |
| 11 | 168–173 | Build `ProxyPoolMcp` with store, balancer, geoip, scheduler handle |
| 12 | 178–279 | **Conditional xray integration** (if `settings.xray.enabled`) |
| 13 | 282–358 | Launch all services via `tokio::spawn` |
| 14 | 361–370 | `tokio::select!` — wait for any service to stop |

---

## Service Composition

All services run concurrently in the same tokio runtime:

```
┌─────────────────────────────────────────────────┐
│                  proxy-server                    │
│                                                  │
│  ┌──────────────┐  ┌──────────────────────────┐ │
│  │  Scheduler    │  │  WarpHealthChecker       │ │
│  │  (fetch+val)  │  │  (periodic probe)        │ │
│  └──────────────┘  └──────────────────────────┘ │
│  ┌──────────────┐  ┌──────────────────────────┐ │
│  │  API (axum)   │  │  Gateway (SOCKS5/HTTP)   │ │
│  └──────────────┘  └──────────────────────────┘ │
│  ┌──────────────┐  ┌──────────────────────────┐ │
│  │  MCP Server   │  │  Sub Refresh Loop        │ │
│  └──────────────┘  └──────────────────────────┘ │
│  ┌──────────────────────────────────────────────┐│
│  │  Xray (conditional)                          ││
│  │  ├─ XrayProcess (subprocess supervisor)      ││
│  │  ├─ XrayClient (gRPC reconnect loop)         ││
│  │  └─ OutboundSync (pending→active sync)       ││
│  └──────────────────────────────────────────────┘│
└─────────────────────────────────────────────────┘
```

### Shared State (Arc)

All components share the same `Arc<ProxyStore>`. Other shared state:

| Component | Shared via | Consumers |
|-----------|-----------|-----------|
| `ProxyStore` | `Arc<ProxyStore>` | Scheduler, API, Gateway, MCP, Sub, Xray sync |
| `WarpBalancer` | `Arc<WarpBalancer>` | Gateway selector, MCP, Health checker |
| `GeoIPLookup` | `Arc<Mutex<GeoIPLookup>>` | Gateway selector, MCP |
| `SchedulerHandle` | `Clone` (wraps `mpsc::Sender`) | API, MCP |
| `xray_active_count` | `Arc<AtomicUsize>` | API, Xray sync |

---

## Conditional Xray Integration

The entire xray subsystem (lines 178–279) is gated behind `settings.xray.enabled`:

```rust
if settings.xray.enabled {
    // 1. Generate bootstrap config → temp file
    // 2. Start xray-core process + supervisor (tokio::spawn)
    // 3. Create PortManager (port range allocation)
    // 4. Create XrayClient (gRPC + watch channel for connection state)
    // 5. Initial gRPC connect (may fail — reconnect loop retries)
    // 6. Create PendingStore (separate Redis connection)
    // 7. Create OutboundSync
    // 8. Spawn reconnect loop
    // 9. Spawn outbound sync loop
}
```

When xray is disabled, `xray_sync_handle` and `xray_supervisor_handle` are `None`,
and the `tokio::select!` branch uses `std::future::pending()` to skip them.

---

## Naming Conventions

- **Variables**: `snake_case`, descriptive (`warp_instances_arc`, `xray_shutdown_for_reconnect`).
- **Task handles**: `{service}_handle` / `{service}_task` (e.g., `scheduler_task`, `health_handle`).
- **Config clones**: When a sub-config needs to be moved into a spawned task, clone it before the spawn: `let sub_config = settings.subscription.clone();`.

---

## Adding a New Top-Level Service

Checklist:

1. Add the crate to `Cargo.toml` `[dependencies]`
2. Import and build the component in `main.rs`
3. Clone any needed `Arc` references before spawning
4. `tokio::spawn` the service's async run loop
5. Add a `tokio::select!` branch for the new handle
6. Add config fields to `Settings` (in `proxy-core::config`) with defaults
7. Document the new service in this spec's Service Composition table
