# PRD: gateway-http-connect-fallback-v1

## Goal

Make overseas HTTP CONNECT gateway traffic recover through valid fallback
exits when WARP is unavailable or slow. The immediate production bug is that
HTTP CONNECT can select an HTTP pool proxy but the gateway currently treats all
pool proxies as SOCKS5 upstreams.

## Background And Evidence

- Live first-layer E2E showed REST and MCP read-only business checks passing,
  while overseas HTTP CONNECT to `httpbin.org:80` returned `502` or timed out.
- Live metrics showed `http_connect.warp.success=0` and
  `http_connect.warp.failure=21`; `http_connect.free_pool.success=1` and
  `http_connect.free_pool.failure=17`.
- Domestic HTTP CONNECT was stable because it selected direct routing.
- SOCKS5 overseas was more successful because the SOCKS5 handler selects
  SOCKS5 pool proxies.
- `crates/proxy-gateway/src/http_connect.rs:49` calls
  `select_with_trace(host, "http")`.
- `crates/proxy-core/src/route_debug.rs:546` uses that protocol to select an
  HTTP pool proxy.
- `crates/proxy-gateway/src/upstream.rs:125` through
  `crates/proxy-gateway/src/upstream.rs:127` routes every `Upstream::Proxy`
  through `connect_via_socks5`, regardless of `proxy.protocol`.
- The gateway handlers do not bound each upstream attempt, so a slow WARP
  attempt can consume the client timeout before later fallback exits are tried.

## Requirements

### R1: Protocol-aware pool proxy upstreams

`Upstream::Proxy(proxy)` must connect using a method compatible with
`proxy.protocol`:

- HTTP/HTTPS pool proxies use HTTP CONNECT upstream handshake.
- SOCKS5 pool proxies use SOCKS5 handshake.
- Unsupported proxy protocols return a structured error.

### R2: Bounded gateway upstream attempts

HTTP CONNECT and SOCKS5 gateway handlers must apply a per-candidate timeout
around upstream connection attempts so WARP or a bad pool proxy cannot block
fallback for the full client timeout.

### R3: Preserve fallback semantics

When WARP fails or times out for overseas routes, the gateway should continue
to xray/free_pool/no_proxy according to the existing candidate order. This task
does not change route ordering. Within the `free_pool` exit, the selector
should provide a bounded set of concrete pool proxy candidates so one bad pool
proxy does not immediately end the business request.

### R4: Focused regression coverage

Add local tests proving:

- HTTP pool proxies receive HTTP CONNECT handshakes.
- SOCKS5 pool proxies still receive SOCKS5 handshakes.
- Timeout/failure on an early candidate can fall through to a later candidate.
- Multiple concrete `free_pool` proxy candidates can be tried under the same
  exit label.

### R5: No live mutation during implementation

Implementation and validation must not SSH, refresh pools, delete proxies, or
mutate dev state. Live checks, if run after push/update, should use public
gateway/API/MCP surfaces only.

## Acceptance Criteria

- [x] Local gateway tests cover HTTP CONNECT via an HTTP upstream proxy.
- [x] Local gateway tests cover SOCKS5 upstream behavior remains intact.
- [x] Local gateway or core tests cover bounded fallback after a failed/slow
      candidate.
- [x] Local core tests cover multiple weighted pool candidates without
      replacement.
- [x] `cargo test -p proxy-gateway` passes.
- [x] Relevant route/gateway/core tests pass.
- [x] No task changes are made to `.codex/config.toml`.

## Out Of Scope

- Reordering overseas route candidates.
- WARP endpoint optimizer changes.
- Full WARP failure hardening or balancer health feedback.
- Mutating live dev pool contents.
