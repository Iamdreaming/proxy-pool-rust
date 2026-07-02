# Logging Guidelines — proxy-xray

> How logging is done in the proxy-xray crate.

---

## Overview

proxy-xray uses `tracing` macros exclusively (never `log`). The crate manages a subprocess and gRPC connection, so logging focuses on lifecycle events, connection state transitions, and sync cycle results.

---

## Log Levels

| Level | When to Use | Examples |
|-------|-------------|---------|
| `error!` | Unrecoverable operation failure that requires intervention or retry | xray-core restart failed after backoff |
| `warn!` | Recoverable issue that may indicate a problem | gRPC connection lost, port exhaustion, add_inbound failed, pending store read failed |
| `info!` | Normal operational milestone | xray-core started, gRPC connected, node activated, stale node removed, sync cycle complete, supervisor started/stopped |
| `debug!` | Detailed operational detail for troubleshooting | removed inbound/outbound tag, CLI API command succeeded |

### Decision Rules

- If the system automatically recovers (reconnect, restart, skip node) → `warn!`
- If recovery fails and backoff increases → `error!`
- If a normal operation completes successfully → `info!`
- If the detail is only useful when debugging a specific issue → `debug!`
- Never use `trace!` — this crate does not have hot paths that need sub-debug visibility

---

## Structured Logging

### Current Pattern: Inline Format Strings

The crate uses `tracing` macros with inline format strings:

```rust
tracing::info!(
    "xray-core started: {} -c {} (api_port={api_port})",
    binary_path,
    config_path.display()
);

tracing::warn!(
    "xray-core died, restarting in {:.1}s (restart #{})",
    backoff_secs,
    self.restart_count() + 1
);

tracing::info!(
    "outbound_sync: cycle complete -- added: {}, removed: {}, failed: {}, total_active: {}",
    stats.added, stats.removed, stats.failed, stats.total_active
);
```

### Preferred Pattern: Structured Fields

For new code, prefer `tracing` structured fields for machine-parseable logs:

```rust
// PREFERRED — structured fields
tracing::info!(
    tag = %node_config.tag,
    local_port = local_port,
    "outbound_sync: activated node"
);

// ACCEPTABLE — inline format (existing code style)
tracing::info!(
    "outbound_sync: activated {} -> local port {}",
    node_config.tag,
    local_port
);
```

Do not mix styles within a single module. If a file already uses inline format strings consistently, follow that style for consistency.

---

## What to Log

### Subprocess Lifecycle

| Event | Level | Fields |
|-------|-------|--------|
| xray-core started | `info!` | binary_path, config_path, api_port |
| xray-core died (detected by supervisor) | `warn!` | backoff_secs, restart_count |
| xray-core restart succeeded | `info!` | restart_count |
| xray-core restart failed | `error!` | error message |
| xray-core killed (shutdown) | `info!` | — |
| Supervisor started | `info!` | — |
| Supervisor shutting down | `info!` | — |

### gRPC Connection

| Event | Level | Fields |
|-------|-------|--------|
| gRPC connected | `info!` | api_addr |
| gRPC connect failed | `warn!` | error message |
| gRPC connection lost (Unavailable) | `warn!` | operation context (remove_inbound/remove_outbound) |
| gRPC health check failed | `warn!` | — |
| gRPC reconnected | `info!` | — |
| gRPC reconnect failed | `warn!` | error message, backoff_secs |
| Reconnect loop started | `info!` | — |
| Reconnect loop shutting down | `info!` | — |

### Outbound Sync

| Event | Level | Fields |
|-------|-------|--------|
| Sync started | `info!` | sync_interval_sec |
| Node activated | `info!` | tag, local_port |
| Stale node removed | `info!` | tag |
| Sync cycle complete | `info!` | added, removed, failed, total_active |
| Pending store read failed | `warn!` | label, error message |
| Add inbound/outbound failed | `warn!` | error message |
| Remove inbound/outbound failed | `warn!` | error message |
| Proxy store add failed | `warn!` | error message |
| Port exhaustion | `warn!` | — |
| Max active nodes reached | `warn!` | max_active_nodes |
| Xray disconnected (skipping sync) | `debug!` | — |
| Xray reconnected (immediate sync) | `info!` | — |
| Sync stopped | `info!` | — |

### CLI API Operations

| Event | Level | Fields |
|-------|-------|--------|
| CLI command succeeded | `debug!` | subcommand (adi/ado) |
| CLI command failed | `warn!`/`error!` | subcommand, exit_code, stderr |

---

## What NOT to Log

### Sensitive Data — NEVER Log

- **Proxy passwords**: Shadowsocks passwords, Trojan passwords — these appear in `SubscriptionProxy` fields and `outbound_json`
- **VMess UUIDs**: User IDs in VMess configs
- **Full outbound JSON**: Contains credentials; log only the tag and port

```rust
// FORBIDDEN — logs credentials
tracing::info!("outbound config: {:?}", node_config.outbound_json);

// REQUIRED — log only identifying info
tracing::info!("outbound_sync: activated {} -> local port {}", node_config.tag, local_port);
```

### Excessive Noise — Do Not Log

- **Every health check success**: Health checks run every 5s when connected; only log failures
- **Port allocation details per port**: Log only exhaustion events
- **Temp file paths**: Internal implementation detail, not useful in logs
- **gRPC request/response bodies**: Too verbose; log only the operation tag and outcome

### Acceptable at Debug Level Only

- Individual inbound/outbound tag removals
- CLI API command success confirmations
- Connection state check results during sync

---

## Log Message Prefix Convention

All log messages from this crate use a module prefix for easy filtering:

- `outbound_sync:` — messages from `outbound_sync.rs`
- `xray-core` / `xray gRPC` / `xray supervisor` — messages from `process.rs` and `xray_client.rs`

No prefix is used for `config_gen.rs` and `port_manager.rs` as they are stateless/utility modules with no logging.
