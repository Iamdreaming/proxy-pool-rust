# proxy-gateway — Backend Development Guidelines

> Best practices for backend development in the proxy-gateway crate.

---

## Overview

proxy-gateway is the HTTP/SOCKS5 proxy gateway in pure Rust. It accepts TCP connections, sniffs the protocol (SOCKS5 vs HTTP CONNECT), selects an upstream proxy via a multi-layer decision chain (Router → GeoIP → fallback), and tunnels traffic bidirectionally.

Key responsibilities:
- Protocol detection (single-byte peek: `0x05` = SOCKS5, else HTTP CONNECT)
- Upstream selection with smart fallback (pool → WARP → Xray → NoProxy)
- SOCKS5 RFC 1928 server-side handling (method negotiation, CONNECT only)
- HTTP CONNECT proxying
- WARP chain tunneling (pool proxy → WARP → target, two-level SOCKS5)
- Bidirectional byte relay between client and upstream

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Directory Structure](./directory-structure.md) | Module organization and file layout | Done |
| ~~Database Guidelines~~ | Not applicable — gateway is stateless, no database | N/A |
| [Error Handling](./error-handling.md) | Error types, handling strategies, log-and-continue pattern | Done |
| [Quality Guidelines](./quality-guidelines.md) | Code standards, forbidden patterns, testing | Done |
| [Logging Guidelines](./logging-guidelines.md) | Structured logging, log levels | Done |

---

## Architecture Summary

```
Client TCP
    │
    ▼
ProxyGateway::run()  ← TCP accept loop, spawns per-connection tasks
    │
    ▼
handle_connection()  ← peek first byte: 0x05 → SOCKS5, else HTTP CONNECT
    │
    ├── socks5::handle()     ← RFC 1928: negotiate → parse request → select upstream → relay
    └── http_connect::handle() ← parse CONNECT → select upstream → relay
    │
    ▼
UpstreamSelector::select()  ← Router → GeoIP → fallback chain
    │
    ├── Direct           → TcpStream::connect(target)
    ├── Proxy(http/https) → connect_via_http_proxy(proxy, target)
    ├── Proxy(socks5)     → connect_via_socks5(proxy, target)
    ├── Proxy(socks4)     → unsupported → failure/fallback
    ├── Warp{id,port}    → connect_via_socks5(127.0.0.1:port, target)
    ├── Xray{port}       → connect_via_socks5(127.0.0.1:port, target)
    ├── WarpChain{..}    → connect_via_warp_chain(proxy, warp_port, target)
    └── NoProxy          → 502 / SOCKS5 reply 0x05
```

---

**Language**: All documentation should be written in **English**.
