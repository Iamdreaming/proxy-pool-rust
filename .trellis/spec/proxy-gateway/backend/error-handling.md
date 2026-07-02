# Error Handling

> How errors are handled in proxy-gateway.

---

## Overview

proxy-gateway uses `anyhow::Result<()>` exclusively for all fallible functions. There are **no custom error types** in this crate. The gateway follows a **log-and-continue** pattern: connection errors are logged and the client receives an appropriate error response, but the accept loop never crashes.

---

## Error Types

### What is used

- `anyhow::Result<T>` — all fallible public and private functions
- `anyhow::bail!()` — for early returns on protocol violations or upstream failures
- `anyhow::anyhow!()` — for constructing ad-hoc errors

### What is NOT used

- No `thiserror` derive in this crate (despite being in `Cargo.toml` as a workspace dependency)
- No custom error enum
- No error type hierarchy

### Rationale

The gateway is a connection-handling service, not a library. Callers don't need to match on specific error variants — they just need to know "this connection failed." `anyhow` keeps the code simple and avoids boilerplate for errors that are always handled the same way (log + send error reply).

---

## Error Handling Patterns

### Pattern 1: Accept loop — never crash

The `run()` accept loop wraps each connection in a spawned task. If `handle_connection` fails, the error is logged at debug level and the task ends. The loop itself only fails on `listener.accept()` errors (bind failure, etc.).

```rust
loop {
    let (stream, client_addr) = listener.accept().await?;
    let gateway = self.clone();
    tokio::spawn(async move {
        if let Err(e) = gateway.handle_connection(stream, client_addr).await {
            tracing::debug!("connection error from {client_addr}: {e}");
        }
    });
}
```

**Rule**: Never propagate connection-level errors out of the accept loop. Log them and move on.

### Pattern 2: Upstream failure — log + send error reply + return Ok(())

When an upstream connection fails, the handler:
1. Logs the failure at `warn!` level
2. Sends an error response to the client (HTTP 502 or SOCKS5 reply code 0x05)
3. Returns `Ok(())` — the connection is "handled" even though it failed

```rust
Err(e) => {
    tracing::warn!("HTTP CONNECT: SOCKS5 via {} failed: {e}", upstream_addr);
    let resp = "HTTP/1.1 502 Bad Gateway\r\n\r\n";
    stream.write_all(resp.as_bytes()).await?;
}
```

**Rule**: A failed upstream is not a bug. It's a normal operational event. Return `Ok(())` after sending the error reply.

### Pattern 3: Protocol violation — bail or send error reply

For malformed client input:
- SOCKS5 wrong version: `bail!("not a SOCKS5 connection")` — propagates as `Err`, caught by the spawn task
- SOCKS5 unsupported command: send reply code 0x07, return `Ok(())`
- SOCKS5 unsupported address type: send reply code 0x08, return `Ok(())`
- HTTP non-CONNECT: send 400 Bad Request, return `Ok(())`

**Rule**: Distinguish between "client is broken" (bail/Err) and "client asked for something we don't support" (error reply + Ok).

### Pattern 4: Empty/zero-length reads — silent Ok(())

If the client disconnects before sending any data (peek returns 0 bytes, or read returns 0), return `Ok(())` silently. This is normal — clients connect and disconnect all the time.

```rust
let n = stream.peek(&mut buf).await?;
if n == 0 {
    return Ok(());
}
```

### Pattern 5: Bidirectional copy errors — debug log only

Errors during the relay phase (after successful upstream connection) are logged at `debug!` level. These are expected when either side disconnects.

```rust
tokio::select! {
    r = client_to_server => { if let Err(e) = r { tracing::debug!("client→server error: {e}"); } }
    r = server_to_client => { if let Err(e) = r { tracing::debug!("server→client error: {e}"); } }
}
```

---

## Error Responses to Clients

| Protocol | Scenario | Response |
|----------|----------|----------|
| HTTP CONNECT | Non-CONNECT request | `HTTP/1.1 400 Bad Request\r\n\r\n` |
| HTTP CONNECT | Empty target | `HTTP/1.1 400 Bad Request\r\n\r\n` |
| HTTP CONNECT | Upstream connect failed | `HTTP/1.1 502 Bad Gateway\r\n\r\n` |
| HTTP CONNECT | NoProxy available | `HTTP/1.1 502 Bad Gateway\r\n\r\n` |
| SOCKS5 | Wrong version | Connection dropped (bail) |
| SOCKS5 | No acceptable auth method | `[0x05, 0xFF]` + bail |
| SOCKS5 | Unsupported command | Reply code 0x07 (command not supported) |
| SOCKS5 | Unsupported address type | Reply code 0x08 (address type not supported) |
| SOCKS5 | Upstream connect failed | Reply code 0x05 (connection refused) |
| SOCKS5 | NoProxy available | Reply code 0x05 (connection refused) |

---

## Common Mistakes

1. **Returning Err from a handler for upstream failures** — This causes the spawned task to log at debug level, but the client gets no error response. Always send an error reply first, then return `Ok(())`.

2. **Using `?` on `stream.write_all` after an upstream failure** — If the client has already disconnected, writing the error reply will fail. This is acceptable (the `?` will propagate to the spawn task which logs at debug), but be aware that the error reply is best-effort.

3. **Logging upstream failures at `error!` level** — Upstream failures are operational events, not bugs. Use `warn!` for upstream failures and `debug!` for per-connection errors.

4. **Forgetting to handle `NoProxy`** — Every upstream match must handle `NoProxy` by sending an error reply. Missing this arm causes a compile error (exhaustive match), but if you use `_` catch-all, you might silently drop the error.
