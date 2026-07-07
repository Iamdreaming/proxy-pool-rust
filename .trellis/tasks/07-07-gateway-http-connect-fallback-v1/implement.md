# Implementation Plan: gateway-http-connect-fallback-v1

## Checklist

1. [x] Load `trellis-before-dev` and read gateway/core/integration specs.
2. [x] Inspect `Proxy` / `Protocol` model to confirm exact enum variants.
3. [x] Implement HTTP CONNECT upstream helper in `proxy-gateway::upstream`.
4. [x] Dispatch `Upstream::Proxy` based on `proxy.protocol`.
5. [x] Add per-candidate timeout in `http_connect` and `socks5` handlers.
6. [x] Add focused async tests for HTTP proxy upstream and SOCKS5 upstream.
7. [x] Add fallback/timeout coverage where feasible without live network.
8. [x] Add runtime WARP failure feedback from gateway attempts into
   `WarpBalancer::mark_failed(id)`.
9. [x] Preserve WARP instance id in `Upstream::Warp`.
10. [x] Add focused WARP balancer failure-marking coverage.
11. [x] Run local verification:
   - `cargo fmt --all`
   - `cargo test -p proxy-gateway`
   - `cargo test -p proxy-core route_debug`
   - `cargo test -p proxy-core warp::balancer`
   - `cargo clippy -p proxy-gateway -- -D warnings`
12. [x] Update task PRD acceptance and gateway/core specs.

## Implementation Notes

- `connect_to_upstream()` now treats `Upstream::Proxy(proxy)` as a
  protocol-aware dispatch point:
  - HTTP/HTTPS proxies use an upstream HTTP CONNECT handshake.
  - SOCKS5 proxies keep the existing SOCKS5 handshake.
  - SOCKS4 proxies return an unsupported-protocol error.
- HTTP CONNECT and SOCKS5 handlers now wrap each candidate connect attempt in
  the shared upstream timeout helper. Timeout is currently a conservative
  gateway-local constant (`5s`).
- The HTTP CONNECT upstream helper reads proxy response headers without
  consuming bytes that belong to the established tunnel.
- `ProxyStore` now supports weighted random candidate selection without
  replacement, and `UpstreamSelector` expands `free_pool` into up to four
  concrete proxy candidates while preserving the high-level route order.
- `UpstreamSelector` now expands `free_pool` into up to four concrete proxy
  candidates to improve live-business success probability while keeping
  per-request fallback bounded.
- `Upstream::Warp` now carries the WARP instance id as well as the SOCKS5 port.
  Gateway handlers report concrete connection failures back through
  `UpstreamSelector::record_upstream_attempt()`, which marks failed WARP
  instances unhealthy in the in-process `WarpBalancer`.
- `WarpBalancer` keeps a 300-second business-failure cooldown per WARP
  instance, so periodic health checks cannot immediately reintroduce a WARP
  instance that just failed real gateway traffic.
- Route ordering is intentionally unchanged.

## Verification Results

- `cargo fmt --all` passed.
- `cargo test -p proxy-gateway` passed: 14 tests.
- `cargo test -p proxy-core route_debug` passed: 5 tests.
- `cargo test -p proxy-core weighted_random_choices` passed: 2 tests.
- `cargo test -p proxy-api route_test` passed: 2 tests.
- `cargo test -p proxy-mcp route_test` passed: 2 tests.
- `cargo clippy -p proxy-gateway -- -D warnings` passed.
- `cargo clippy -p proxy-core -p proxy-gateway -- -D warnings` passed.
- After WARP runtime feedback changes:
  - `cargo fmt --all --check` passed.
  - `cargo test -p proxy-core route_debug` passed: 5 tests.
  - `cargo test -p proxy-core warp::balancer` passed: 1 test.
  - `cargo test -p proxy-gateway` passed: 14 tests.
  - `cargo test -p proxy-api route_test` passed: 2 tests.
  - `cargo test -p proxy-mcp route_test` passed: 2 tests.
  - `cargo clippy -p proxy-core -p proxy-gateway -- -D warnings` passed.
  - `cargo check --workspace` passed.
- After WARP business-failure cooldown:
  - `cargo fmt --all --check` passed.
  - `cargo test -p proxy-core warp::balancer` passed: 1 test.
  - `cargo test -p proxy-core route_debug` passed: 5 tests.
  - `cargo test -p proxy-gateway` passed: 14 tests.
  - `cargo test -p proxy-api route_test` passed: 2 tests.
  - `cargo test -p proxy-mcp route_test` passed: 2 tests.
  - `cargo clippy -p proxy-core -p proxy-gateway -- -D warnings` passed.
  - `cargo check --workspace` passed.

## Rollback Points

- If HTTP CONNECT upstream support needs larger proxy-auth handling, implement
  no-auth CONNECT first and document proxy-auth as out of scope.
- If timeout behavior needs configuration, start with a local constant and
  promote to config in a later task only if required.
- If WARP runtime feedback disables WARP too aggressively, shorten the
  balancer's business-failure cooldown first. If needed, revert the
  `Upstream::Warp { id, .. }` feedback path and keep HTTP proxy upstream plus
  free-pool multi-candidate fallback.
