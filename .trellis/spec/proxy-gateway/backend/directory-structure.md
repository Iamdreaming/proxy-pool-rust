# Directory Structure

> How backend code is organized in proxy-gateway.

---

## Overview

proxy-gateway is a flat-module crate with four source files. Each file owns a distinct protocol or responsibility layer. There is no sub-module nesting beyond the top level.

---

## Directory Layout

```
crates/proxy-gateway/
├── Cargo.toml
└── src/
    ├── lib.rs           # ProxyGateway struct, run() (accept loop), handle_connection() (protocol sniff)
    ├── upstream.rs      # Upstream enum, UpstreamSelector, protocol-aware connect helpers, parse_target_addr()
    ├── http_connect.rs  # HTTP CONNECT handler, bidirectional_copy
    └── socks5.rs        # SOCKS5 (RFC 1928) handler, reply encoding, bidirectional_copy
```

---

## Module Organization

### `lib.rs` — Entry point and accept loop

- Declares `mod http_connect`, `mod socks5`, `mod upstream`
- Re-exports: `UpstreamSelector`, `connect_via_http_proxy`, `connect_via_socks5`, `connect_via_warp_chain`, `socks5_handshake_on_stream`
- `ProxyGateway` struct: holds `GatewaySettings` + `Arc<UpstreamSelector>`
- `run()`: binds TCP listener, spawns a tokio task per connection
- `handle_connection()`: single-byte peek to dispatch protocol

### `upstream.rs` — Upstream selection and protocol-aware connect utilities

- `Upstream` enum: `Direct`, `Proxy(Proxy)`, `Warp{id, socks5_port}`, `Xray{local_socks5_port}`, `WarpChain{proxy, socks5_port}`, `NoProxy`
- `UpstreamSelector`: holds `ProxyStore`, `WarpBalancer`, `Router`, `GeoIPLookup`
- `select()` method: Router match → GeoIP auto-split → generic fallback chain
- Helper methods: `try_pool()`, `try_warp()`, `try_xray()`
- Free functions: `socks5_handshake_on_stream()`, `connect_via_http_proxy()`, `connect_via_socks5()`, `connect_via_warp_chain()`, `parse_target_addr()`
- `connect_to_upstream()` must dispatch `Upstream::Proxy(proxy)` by `proxy.protocol`: HTTP/HTTPS use HTTP CONNECT, SOCKS5 uses SOCKS5 handshake, SOCKS4 is rejected as unsupported.
- Tests: address parsing (IPv4/IPv6/domain), variant construction, SOCKS5 request format, loopback HTTP CONNECT/SOCKS5 upstream dispatch, HTTP CONNECT tunnel byte preservation, slow-upstream timeout

### `http_connect.rs` — HTTP CONNECT protocol handler

- `handle()`: reads CONNECT request, selects upstream, connects, relays
- `bidirectional_copy()`: `tokio::select!` on two `io::copy` directions
- Error responses: 400 Bad Request, 502 Bad Gateway

### `socks5.rs` — SOCKS5 protocol handler (RFC 1928)

- `handle()`: method negotiation (no-auth only), request parsing (CONNECT only, IPv4/IPv6/domain), upstream selection, relay
- `socks5_reply()`: minimal reply builder (always IPv4 0.0.0.0:0)
- `socks5_reply_from_addr()`: reply builder using actual `SocketAddr`
- `bidirectional_copy()`: same pattern as http_connect (independent copy)

---

## Naming Conventions

| Item | Convention | Example |
|------|-----------|---------|
| Source files | `snake_case`, one word or compound | `http_connect.rs`, `socks5.rs`, `upstream.rs` |
| Public structs | `PascalCase` | `ProxyGateway`, `UpstreamSelector` |
| Public enums | `PascalCase` enum, `PascalCase` variants | `Upstream::WarpChain { .. }` |
| Public functions | `snake_case` | `connect_via_http_proxy`, `connect_via_socks5`, `socks5_handshake_on_stream` |
| Private helpers | `snake_case` | `bidirectional_copy`, `socks5_reply` |
| Protocol constants | Hex literals inline | `0x05`, `0x01`, `0x03`, `0xFF` |

---

## How to Add New Upstream Types

1. Add a variant to the `Upstream` enum in `upstream.rs`
2. Add the matching arm in both `http_connect.rs::handle()` and `socks5.rs::handle()`
3. If the new type needs a connect helper, add a free function in `upstream.rs` and re-export from `lib.rs`
4. Add a `try_*` method on `UpstreamSelector` if it participates in the fallback chain
5. Update the fallback chain in `select()` if needed

---

## How to Add New Protocol Support

1. Create a new `src/<protocol>.rs` file with a `pub async fn handle(stream, client_addr, selector)` function
2. Add `mod <protocol>;` to `lib.rs`
3. Update `handle_connection()` to detect and dispatch the new protocol
4. Follow the same pattern: parse request → select upstream → connect → relay → error reply

---

## Examples

- **Protocol handler pattern**: `socks5.rs` is the most complete example — negotiation → request parsing → upstream selection → connect → relay → error reply, all following RFC 1928
- **Upstream integration**: `http_connect.rs` shows the simplest upstream dispatch (no method negotiation, just CONNECT parsing)
- **Connect helper**: `upstream.rs::connect_via_warp_chain()` shows multi-hop SOCKS5 tunneling
