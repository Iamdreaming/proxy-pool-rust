# Verification: Gateway Route Debugging

## 2026-07-07 Closeout

Local focused checks:

- `cargo test -p proxy-core route_debug` passed: 13 tests.
- `cargo test -p proxy-api route_test` passed: 2 tests.

Read-only dev checks, no SSH and no mutation:

- `GET /api/status` returned `git_hash=6920a96`, `readyz=ok`, pool total 586, WARP 3/3 healthy, xray disabled.
- `GET /api/routes/test?host=api.openai.com&protocol=http` returned `matched_reason=business_domain_overseas`, selected `warp`, and xray unavailable because no active xray node exists.
- `GET /api/routes/test?host=google.com&protocol=http` returned `matched_reason=geoip_overseas`, GeoIP country `US`, selected `warp`.
- `GET /api/routes/test?host=baidu.com&protocol=http` returned `matched_reason=geoip_domestic`, GeoIP country `CN`, selected `direct`.
- `GET /api/routes/test?host=github.com&protocol=http` returned `matched_reason=direct_reachable_domain`, selected `direct`.
- `GET /api/metrics` included `proxy_gateway_route_attempts_total` with protocol, exit, and status labels.

Notes:

- Dev currently runs `6920a96`, while local `main` is ahead of `origin/main`; this closeout does not trigger deployment or `update_service`.
- The optional debug header remains intentionally deferred. Existing API/MCP dry-run and metrics cover the accepted operator diagnostics.
