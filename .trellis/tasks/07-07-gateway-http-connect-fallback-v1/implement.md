# Implementation Plan: gateway-http-connect-fallback-v1

## Checklist

1. [x] Load `trellis-before-dev` and read gateway/core/integration specs.
2. [x] Inspect `Proxy` / `Protocol` model to confirm exact enum variants.
3. [x] Implement HTTP CONNECT upstream helper in `proxy-gateway::upstream`.
4. [x] Dispatch `Upstream::Proxy` based on `proxy.protocol`.
5. [x] Add per-candidate timeout in `http_connect` and `socks5` handlers.
6. [x] Add focused async tests for HTTP proxy upstream and SOCKS5 upstream.
7. [x] Add fallback/timeout coverage where feasible without live network.
8. [x] Run local verification:
   - `cargo fmt --all`
   - `cargo test -p proxy-gateway`
   - `cargo test -p proxy-core route_debug`
   - `cargo clippy -p proxy-gateway -- -D warnings`
9. [x] Update task PRD acceptance and gateway specs.

## Implementation Notes

- `connect_to_upstream()` now treats `Upstream::Proxy(proxy)` as a
  protocol-aware dispatch point:
  - HTTP/HTTPS proxies use an upstream HTTP CONNECT handshake.
  - SOCKS5 proxies keep the existing SOCKS5 handshake.
  - SOCKS4 proxies return an unsupported-protocol error.
- HTTP CONNECT and SOCKS5 handlers now wrap each candidate connect attempt in
  the shared upstream timeout helper. Timeout is currently a conservative
  gateway-local constant (`8s`).
- The HTTP CONNECT upstream helper reads proxy response headers without
  consuming bytes that belong to the established tunnel.
- Route ordering is intentionally unchanged.

## Verification Results

- `cargo fmt --all` passed.
- `cargo test -p proxy-gateway` passed: 14 tests.
- `cargo test -p proxy-core route_debug` passed: 5 tests.
- `cargo clippy -p proxy-gateway -- -D warnings` passed.

## Rollback Points

- If HTTP CONNECT upstream support needs larger proxy-auth handling, implement
  no-auth CONNECT first and document proxy-auth as out of scope.
- If timeout behavior needs configuration, start with a local constant and
  promote to config in a later task only if required.
- If live E2E still fails after protocol fix, separate remaining WARP health
  feedback into a new task rather than expanding this one.
