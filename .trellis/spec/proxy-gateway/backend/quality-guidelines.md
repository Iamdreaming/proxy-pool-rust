# Quality Guidelines

> Code quality standards for proxy-gateway.

---

## Overview

proxy-gateway is a network-facing service that must stay up under all conditions. Quality standards prioritize robustness (never crash on bad input), correctness (protocol compliance), and testability (pure logic tests without I/O).

---

## Forbidden Patterns

### 1. Never `unwrap()` on network I/O results

Network operations can always fail. Use `?` for propagation or explicit error handling.

```rust
// FORBIDDEN
let n = stream.read(&mut buf).await.unwrap();

// CORRECT
let n = stream.read(&mut buf).await?;
if n == 0 { return Ok(()); }
```

Exception: `unwrap()` is acceptable in tests and on infallible operations like `Ipv4Addr::parse()` in test-only code.

### 2. Never panic in connection handlers

The accept loop must never crash. All connection errors must be caught and logged.

```rust
// FORBIDDEN — will crash the accept loop if the handler panics
tokio::spawn(async move {
    gateway.handle_connection(stream, client_addr).await.unwrap();
});

// CORRECT — errors are logged, task ends gracefully
tokio::spawn(async move {
    if let Err(e) = gateway.handle_connection(stream, client_addr).await {
        tracing::debug!("connection error from {client_addr}: {e}");
    }
});
```

### 3. Never block the accept loop

The accept loop must only do `listener.accept().await` and `tokio::spawn`. No processing, no allocation, no I/O inside the loop body beyond the spawn.

### 4. Never use `log` crate

Use `tracing` macros exclusively (`tracing::info!`, `tracing::warn!`, `tracing::debug!`, `tracing::error!`). The `log` crate is not used in this project.

### 5. Never use `_` catch-all in `Upstream` match arms

Always enumerate all `Upstream` variants explicitly. This ensures compile-time errors when new variants are added.

```rust
// FORBIDDEN — silently ignores new variants
match upstream {
    Upstream::Direct => { ... }
    _ => { ... }
}

// CORRECT — compiler enforces exhaustiveness
match upstream {
    Upstream::Direct => { ... }
    Upstream::Proxy(proxy) => { ... }
    Upstream::Warp { socks5_port } => { ... }
    Upstream::Xray { local_socks5_port } => { ... }
    Upstream::WarpChain { proxy, socks5_port } => { ... }
    Upstream::NoProxy => { ... }
}
```

Exception: `Warp` and `Xray` may be combined with `|` since they share the same handling logic (both connect to a local SOCKS5 port).

### 6. Never store connection state in `ProxyGateway`

`ProxyGateway` holds only configuration (`GatewaySettings`) and shared state (`Arc<UpstreamSelector>`). Per-connection state must be local to the handler function.

---

## Required Patterns

### 1. Protocol handlers must follow the parse → select → connect → relay → error-reply pattern

Every protocol handler (`http_connect::handle`, `socks5::handle`) must:
1. Parse the client request
2. Select upstream via `UpstreamSelector::select()`
3. Connect to the upstream
4. Send success reply to client
5. Relay bidirectionally
6. On any failure: log + send error reply + return `Ok(())`

### 2. All public functions must have doc comments

```rust
/// Connect to a target through a SOCKS5 upstream proxy.
///
/// This establishes a TCP connection to the upstream SOCKS5 proxy,
/// performs the SOCKS5 handshake, and sends a CONNECT request for the target.
/// Returns a TcpStream that is already tunneled to the target.
pub async fn connect_via_socks5(...) -> anyhow::Result<TcpStream> { ... }
```

### 3. Use `tokio::select!` for bidirectional relay

The relay must abort when either direction fails or completes. Never use two sequential `io::copy` calls.

```rust
tokio::select! {
    r = client_to_server => { /* handle */ }
    r = server_to_client => { /* handle */ }
}
```

### 4. SOCKS5 address encoding must handle all three types

IPv4 (ATYP 0x01), domain (ATYP 0x03), and IPv6 (ATYP 0x04) must all be supported in both parsing and encoding. See `upstream.rs::socks5_handshake_on_stream()` and `socks5.rs::handle()`.

### 5. Use `Arc<Self>` for `run()` method

The `run()` method takes `self: Arc<Self>` so it can be cloned into spawned tasks without requiring `Arc` wrapping at the call site.

---

## Testing Requirements

### Test categories

1. **Protocol format tests** — Verify SOCKS5 request/reply byte layouts (IPv4, IPv6, domain)
2. **Address parsing tests** — `parse_target_addr()` for all address types
3. **Variant construction tests** — Ensure `Upstream` variants compile and destructure correctly
4. **No I/O in unit tests** — All tests are pure logic; no TCP connections, no network

### Test location

Tests are inline in each module using `#[cfg(test)] mod tests { ... }` in the same file.

### Required test coverage

- Every `Upstream` variant must have a construction test
- `parse_target_addr()` must be tested for IPv4, IPv6, and domain
- SOCKS5 request encoding must be tested for all three ATYP values
- New public functions must have at least one test

### Running tests

```bash
cargo test -p proxy-gateway
cargo clippy -p proxy-gateway -- -D warnings
```

---

## Code Review Checklist

- [ ] No `unwrap()` on network I/O results (tests excepted)
- [ ] No panics in connection handler paths
- [ ] All `Upstream` variants handled explicitly (no `_` catch-all)
- [ ] Error replies sent to client before returning `Ok(())`
- [ ] `bidirectional_copy` uses `tokio::select!`
- [ ] SOCKS5 address types (IPv4/IPv6/domain) all handled
- [ ] Doc comments on all public items
- [ ] `tracing` used (not `log`)
- [ ] Tests added for new public functions
- [ ] `cargo clippy -p proxy-gateway -- -D warnings` passes
