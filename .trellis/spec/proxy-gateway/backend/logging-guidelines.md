# Logging Guidelines

> How logging is done in proxy-gateway.

---

## Overview

proxy-gateway uses the `tracing` crate for all logging. No `log` crate macros are used. Logging follows a strict level hierarchy based on operational impact: lifecycle events at info, upstream failures at warn, per-connection noise at debug.

---

## Log Levels

### `info!` — Service lifecycle events

Used for events that mark the start or stop of the gateway service. These appear in normal production logs.

```rust
tracing::info!("proxy gateway listening on {addr}");
```

When to use:
- Gateway starts listening
- Gateway shuts down (if applicable)

When NOT to use:
- Per-connection events (too noisy)
- Upstream failures (use `warn!`)

### `warn!` — Upstream failures

Used when an upstream proxy or target connection fails. These indicate that the proxy infrastructure is having issues and may need attention.

```rust
tracing::warn!("HTTP CONNECT: cannot connect to {target}: {e}");
tracing::warn!("HTTP CONNECT: SOCKS5 via {} failed: {e}", upstream_addr);
tracing::warn!("SOCKS5: SOCKS5 chain via {} failed: {e}", upstream_addr);
tracing::warn!("HTTP CONNECT: WarpChain via {}->WARP:{} failed: {e}", proxy.host, socks5_port);
```

When to use:
- Target host unreachable (Direct mode)
- SOCKS5 upstream connection refused or handshake failed
- WARP/Xray endpoint unavailable
- WarpChain tunnel failure

When NOT to use:
- Client disconnections (expected, use `debug!`)
- Protocol violations by the client (use `debug!`)

### `debug!` — Per-connection errors and detailed tracing

Used for events that happen on every connection and are not operationally significant. These are disabled in normal production but useful during development or troubleshooting.

```rust
tracing::debug!("connection error from {client_addr}: {e}");
tracing::debug!("client→server error: {e}");
tracing::debug!("SOCKS5 client→server: {e}");
tracing::debug!("try_xray: failed to query store: {e}");
```

When to use:
- Client disconnected mid-connection
- Bidirectional copy errors (one side closed)
- SOCKS5 method negotiation failed (client doesn't support no-auth)
- Store query failures in `try_xray()`
- Any error that is "normal" in a proxy server handling thousands of connections

When NOT to use:
- Upstream infrastructure failures (use `warn!`)

### `error!` — Not currently used

Reserved for truly unexpected conditions that indicate a bug. If you add `error!` logging, it should only be for states that should never happen (e.g., corrupted internal state, assertion failures in production).

---

## Structured Logging

### Current format

The gateway uses `tracing` macros with inline format strings. There is no structured key-value logging (`tracing::info!` with `fields`).

```rust
// Current style — inline format
tracing::warn!("HTTP CONNECT: SOCKS5 via {} failed: {e}", upstream_addr);
```

### Consistent prefixes

All log messages in protocol handlers include a prefix identifying the protocol and context:

| Module | Prefix format | Example |
|--------|--------------|---------|
| `lib.rs` | No prefix (lifecycle only) | `"proxy gateway listening on {addr}"` |
| `http_connect.rs` | `"HTTP CONNECT: "` | `"HTTP CONNECT: cannot connect to {target}: {e}"` |
| `socks5.rs` | `"SOCKS5: "` | `"SOCKS5: SOCKS5 chain via {} failed: {e}"` |
| `upstream.rs` | `"try_xray: "` | `"try_xray: failed to query store: {e}"` |

**Rule**: New log messages must include the protocol prefix so operators can filter logs by protocol.

---

## What to Log

### Must log

- Gateway bind/listen address (info)
- Upstream connection failures with target and error details (warn)
- WarpChain failures with both proxy and WARP endpoint (warn)

### Should log

- Client disconnection during relay (debug)
- Store query failures in upstream selection (debug)
- Unsupported SOCKS5 commands or address types (debug)

### Optional log

- Each accepted connection with client address (would be too noisy in production, use debug or omit)

---

## What NOT to Log

### Never log

- **Proxy credentials** — If SOCKS5 auth is added in the future, never log username/password
- **Full request payloads** — The gateway only sees CONNECT target addresses and SOCKS5 requests, never body content, but if HTTP full proxy is added, do not log request/response bodies
- **Client IP addresses in production info logs** — Client addresses appear only in debug-level logs. Do not promote them to info/warn unless there is a specific operational need (e.g., rate limiting)

### Caution

- **Target hostnames** — Currently logged in warn messages (e.g., `"cannot connect to {target}"`). This is acceptable for troubleshooting, but be aware that target hostnames may reveal user browsing patterns. In privacy-sensitive deployments, consider redacting or hashing target addresses in logs.

---

## Log Level Decision Flowchart

```
Is this a gateway lifecycle event (start/stop)?
  → YES: info!
  → NO ↓

Is an upstream proxy or target connection failing?
  → YES: warn!
  → NO ↓

Is this a per-connection error (client disconnect, relay error)?
  → YES: debug!
  → NO ↓

Is this a state that should never happen (bug)?
  → YES: error!
  → NO ↓

Don't log it.
```
