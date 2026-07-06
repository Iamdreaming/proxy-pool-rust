# Error Handling — proxy-xray

> How errors are handled in the proxy-xray crate.

---

## Overview

proxy-xray uses `anyhow::Result` exclusively for error propagation. Although `thiserror` is declared in `Cargo.toml`, it is not currently used — all errors are ad-hoc `anyhow::anyhow!()` or `anyhow::bail!()`. The crate has no external API consumers (it is a library used only by `proxy-server`), so structured error types are not required.

The critical error scenarios in this crate are: gRPC connection failures, subprocess crashes, and sync failures. All are handled with retry/backoff patterns rather than fatal errors.

---

## Error Types

### Current State: anyhow-only

```rust
// All errors use anyhow — no custom error enums
anyhow::anyhow!("failed to spawn xray-core: {e}")
anyhow::anyhow!("gRPC connect failed: {e}")
anyhow::bail!("xray gRPC client not connected")
anyhow::anyhow!("gRPC error removing inbound {tag}: {status}")
anyhow::anyhow!("failed to execute xray api {subcommand}: {e}")
```

### When to Introduce thiserror

If proxy-xray ever exposes a public API that callers need to match on, introduce a `thiserror` enum:

```rust
#[derive(Debug, thiserror::Error)]
pub enum XrayError {
    #[error("gRPC connection lost: {0}")]
    GrpcDisconnected(String),
    #[error("xray-core subprocess crashed: {0}")]
    ProcessCrashed(String),
    #[error("port allocation exhausted")]
    PortExhausted,
    #[error("config generation failed for protocol: {0}")]
    ConfigGenerationFailed(String),
}
```

Currently this is **not needed** — all callers use `.await?` and let anyhow propagate.

---

## Error Handling Patterns

### 1. gRPC Transport Errors — Mark Disconnected, Do Not Fail

When a gRPC call returns `tonic::Code::Unavailable`, the client marks itself as disconnected and clears the gRPC client handle. The reconnect loop will restore the connection.

```rust
// xray_client.rs — remove_inbound / remove_outbound
Err(status) => {
    if status.code() == tonic::Code::Unavailable {
        self.connected_tx.send(false).ok();
        self.grpc_client = None;
        tracing::warn!("xray gRPC connection lost (remove_inbound)");
    }
    Err(anyhow::anyhow!("gRPC error removing inbound {tag}: {status}"))
}
```

**Key rule**: Never panic or abort on gRPC errors. Always mark disconnected and let the reconnect loop handle recovery.

### 2. Subprocess Crash — Supervise with Exponential Backoff

The `XrayProcess::supervise()` loop detects process death and restarts with backoff (1s → 2s → 4s → ... → 60s max). On successful restart, backoff resets to 1s.

```rust
// process.rs — supervise loop
match self.restart().await {
    Ok(()) => {
        backoff_secs = 1.0;  // reset on success
    }
    Err(e) => {
        backoff_secs = (backoff_secs * 2.0).min(MAX_BACKOFF);
        tracing::error!("xray-core restart failed: {e}");
    }
}
```

### 3. gRPC Reconnect — Exponential Backoff (1s–30s)

The `XrayClient::reconnect_loop()` uses a separate backoff (1s → 2s → 4s → ... → 30s max). On successful connect, backoff resets.

```rust
// xray_client.rs — reconnect_loop
backoff = Duration::from_secs(1);  // reset on success
// on failure:
backoff = (backoff * 2).min(max_backoff);
```

### 4. Sync Errors — Log and Continue

`OutboundSync::sync_once()` never fails the entire sync cycle on a single node error. Individual failures are logged and counted in `SyncStats::failed`, then the loop continues to the next node.

```rust
// outbound_sync.rs — per-node error handling
if let Err(e) = self.proxy_store.add(&proxy).await {
    tracing::warn!("outbound_sync: failed to store proxy: {e}");
    self.port_manager.release(local_port).await;
    stats.failed += 1;
    continue;  // move to next node
}
```

### 5. CLI API Errors — Fail the Operation, Clean Up Temp File

`execute_cli_api()` writes JSON to a temp file, runs the CLI, and always cleans up the temp file regardless of success or failure.

```rust
// xray_client.rs — execute_cli_api
let _ = std::fs::remove_file(&temp_path);  // always clean up
if !output.status.success() {
    anyhow::bail!("xray api {subcommand} failed with exit code {}: {stderr}", ...);
}
```

### 6. Port Allocation — Return None, Do Not Error

`PortManager::allocate()` returns `Option<u16>` rather than `Result`. Port exhaustion is an expected operational condition, not an error.

---

## Health Check Pattern

The gRPC health check uses a sentinel `remove_inbound("__health_check__")` call. A `NotFound` response means the connection is alive; `Unavailable` means dead.

```rust
// xray_client.rs — health_check
match client.remove_inbound(req).await {
    Ok(_) => true,
    Err(status) => status.code() != tonic::Code::Unavailable,
}
```

---

## Error Propagation Across Components

```
OutboundSync
  ├── PendingStore errors → log + skip label batch
  ├── ConfigGenerator errors → release port + count as failed
  ├── XrayClient add errors → release allocated port, mark lifecycle failed, skip active registration
  ├── XrayClient remove errors → log + continue (stale node still removed from active set)
  └── ProxyStore errors → release port + count as failed

XrayClient
  ├── gRPC Unavailable → mark disconnected + clear client
  ├── gRPC other errors → return Err
  └── CLI errors → return Err (caller logs)

XrayProcess
  ├── Spawn failure → return Err (caller decides)
  ├── Immediate crash → return Err
  └── Runtime crash → supervisor restarts with backoff
```

---

## Common Mistakes

1. **Forgetting to release port on error**: When a node activation fails after port allocation, the port must be released. The `sync_once()` code handles this, but new code paths must follow the same pattern.

2. **Holding write lock during gRPC calls in sync_once()**: The add operations use a read lock (`client.read().await`) because `add_inbound`/`add_outbound` take `&self`. Remove operations require a write lock because they take `&mut self`. Mixing these up causes deadlocks or compilation errors.

3. **Not checking `is_connected()` before gRPC calls**: Always check connection state before attempting gRPC operations. The `connected_rx` watch channel is the source of truth, not `grpc_client.is_some()`.

4. **Panic on subprocess failure**: Never use `unwrap()` on subprocess operations. The process can crash at any time — always handle `Err` or `None` cases gracefully.

5. **Ignoring `tonic::Code::Unavailable` specifically**: Other gRPC error codes (e.g., `NotFound`, `InvalidArgument`) do not indicate connection loss. Only `Unavailable` should trigger the disconnect-and-reconnect flow.
