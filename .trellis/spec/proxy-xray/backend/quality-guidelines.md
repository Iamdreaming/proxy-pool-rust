# Quality Guidelines — proxy-xray

> Code quality standards for the proxy-xray crate.

---

## Overview

proxy-xray follows the workspace-wide quality standards (Edition 2024, `cargo clippy -- -D warnings`, `cargo fmt`) with additional crate-specific rules for async safety, subprocess management, and gRPC interaction.

---

## Forbidden Patterns

### 1. Never `unwrap()` on subprocess or gRPC operations

Subprocesses can crash; gRPC connections can drop. All such operations must return `Result` or handle the `None`/`Err` case.

```rust
// FORBIDDEN
let status = child.try_wait().unwrap();

// REQUIRED
match child.try_wait() {
    Ok(Some(status)) => { /* handle exited */ }
    Ok(None) => { /* still running */ }
    Err(e) => { /* handle error */ }
}
```

### 2. Never hold a write lock across `.await` points unless necessary

`XrayClient` is wrapped in `Arc<RwLock<XrayClient>>`. The reconnect loop acquires write locks briefly for `connect()` and `health_check()`, then drops them. Long-held write locks block all other consumers (sync, API handlers).

```rust
// FORBIDDEN — holding write lock across sleep
let mut guard = client.write().await;
guard.connect().await;
tokio::time::sleep(Duration::from_secs(5)).await; // blocks everyone!

// REQUIRED — scope the lock
let result = {
    let mut guard = client.write().await;
    guard.connect().await
};
// lock released, then sleep
```

### 3. Never panic on port exhaustion

`PortManager::allocate()` returns `Option<u16>`. Callers must handle `None` gracefully — log a warning and skip, not `unwrap()` or `expect()`.

### 4. Never use `log` crate — use `tracing`

The workspace standard is `tracing`. Do not introduce `log::info!`, `log::warn!`, etc.

### 5. Never construct `TypedMessage` manually in Rust

The whole reason for the CLI hybrid approach is that xray's `TypedMessage` protobuf wrapping is complex and error-prone. Add operations go through `xray api adi/ado` CLI which handles this internally. Only remove operations (which just need a tag string) use direct gRPC.

---

## Required Patterns

### 1. Exponential backoff with reset on success

All retry loops must use exponential backoff that resets on success:

```rust
let mut backoff = Duration::from_secs(1);
let max_backoff = Duration::from_secs(30);

// On failure:
backoff = (backoff * 2).min(max_backoff);

// On success:
backoff = Duration::from_secs(1);
```

Current backoff parameters:
- gRPC reconnect: 1s → 2s → 4s → ... → 30s max
- Subprocess supervisor: 1s → 2s → 4s → ... → 60s max

### 2. Watch channel for connection state

Use `tokio::sync::watch` to broadcast connection state. Consumers clone the receiver and use `changed().await` to react to state transitions.

```rust
let (connected_tx, connected_rx) = watch::channel(false);
// On connect:
connected_tx.send(true).ok();
// On disconnect:
connected_tx.send(false).ok();
```

### 3. Shutdown via watch channel

All long-running loops (`supervise`, `reconnect_loop`, `run`) accept a `watch::Receiver<bool>` for shutdown signaling. They exit when the sender is dropped or sends `true`.

```rust
tokio::select! {
    _ = shutdown_rx.changed() => { /* clean shutdown */ }
    _ = tokio::time::sleep(interval) => { /* normal work */ }
}
```

### 4. Port release on error

When a node activation fails after port allocation, always release the port:

```rust
let local_port = match self.port_manager.allocate().await {
    Some(p) => p,
    None => { break; }
};
// ... if anything fails later:
self.port_manager.release(local_port).await;
```

### 5. Temp file cleanup

CLI API operations write temp files. Always clean up regardless of success or failure:

```rust
let _ = std::fs::remove_file(&temp_path); // always, even on error
```

### 6. Doc comments on public items

All public structs, methods, and functions must have `///` doc comments explaining purpose, parameters, and return values.

---

## Testing Requirements

### Unit Tests (current coverage)

| Module | Test Coverage | Notes |
|--------|--------------|-------|
| `config_gen` | High — 9 tests | All protocols (SS, VMess-WS-TLS, VMess-gRPC, Trojan-TLS, Trojan-no-SNI, Basic, Unknown), bootstrap config, inbound/routing structure |
| `port_manager` | High — 6 tests | Sequential allocation, release-reuse, exhaustion, is_allocated, used_count, claim |
| `models` | Minimal — 1 test | Only `SyncStats::default` |
| `process` | None | Hard to unit test subprocess; integration test with mock binary |
| `xray_client` | None | Requires running xray-core; integration test |
| `outbound_sync` | Minimal — 1 test | Only `SyncStats::default` (duplicated from models) |

### Required Test Coverage for New Code

- **Config generation**: Every new protocol variant must have a test in `config_gen` verifying outbound JSON structure, inbound JSON, and routing rule
- **Port management**: Any new `PortManager` method must have a corresponding test
- **Sync logic**: New sync behavior (e.g., different stale detection) needs a test with mock stores

### Integration Test Gaps

The following areas lack automated testing and would benefit from integration tests with a mock xray-core binary:

1. `XrayProcess` start/restart/kill lifecycle
2. `XrayClient` gRPC connect/disconnect/reconnect
3. `OutboundSync` full cycle with mock `PendingStore` and `ProxyStore`

---

## Code Review Checklist

- [ ] No `unwrap()` on subprocess/gRPC/port operations
- [ ] Exponential backoff resets on success
- [ ] Watch channel used for state broadcasting (not polling)
- [ ] Shutdown signal respected in all long-running loops
- [ ] Port released on all error paths after allocation
- [ ] Temp files cleaned up in CLI API operations
- [ ] gRPC `Unavailable` specifically handled (not generic error)
- [ ] Write lock scope minimized — not held across `.await` points unnecessarily
- [ ] New protocol variants have config generation tests
- [ ] `tracing` used (not `log`)
- [ ] Public items have doc comments
