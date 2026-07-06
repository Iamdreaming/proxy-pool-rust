# Implementation Plan: Gateway Route Debugging

## Phase 1: Planning

- [x] Update roadmap to make `gateway-route-debugging` the active task.
- [x] Isolate paused `fetcher-validator-quality` WIP in a git stash.
- [x] Create Trellis task.
- [x] Inspect current router, gateway handlers, API routes, MCP tools, metrics, and integration tests.
- [x] Write PRD, design, and implementation plan.

## Phase 2: Route Decision Model

- [ ] Add route decision and candidate structs with serde support.
- [ ] Add stable exit/status enums.
- [ ] Add tests for serialization and representative candidate orders.

## Phase 3: Traceable Selector

- [ ] Add `select_with_trace(host, protocol)` while preserving `select(host, protocol)`.
- [ ] Record matched route group and matched reason where current router data allows it.
- [ ] Record GeoIP summary when GeoIP is consulted.
- [ ] Record unavailable reasons for pool, WARP, xray, and no-proxy cases.
- [ ] Add selector tests for explicit direct, explicit free_pool, overseas fallback, and no-upstream.

## Phase 4: Gateway Runtime Attempts

- [ ] Update HTTP CONNECT handler to use traceable selection.
- [ ] Update SOCKS5 handler to use traceable selection.
- [ ] Add structured attempt logging.
- [ ] Implement retry-across-candidates before client success response if feasible within current handler structure.
- [ ] Add gateway tests for success/failure and retry behavior if runtime retry is implemented.

## Phase 5: API And MCP

- [ ] Add route dry-run endpoint to `proxy-api`.
- [ ] Add `route_test` tool to `proxy-mcp`.
- [ ] Ensure API/MCP share selector logic instead of duplicating route decisions.
- [ ] Add serialization/deserialization tests.

## Phase 6: Metrics

- [ ] Add gateway route outcome metrics with stable names and labels.
- [ ] Render metrics through `/api/metrics`.
- [ ] Add metrics unit tests.

## Phase 7: Verification

- [ ] `cargo fmt --all --check`
- [ ] `cargo test -p proxy-core --lib`
- [ ] `cargo test -p proxy-gateway --lib`
- [ ] `cargo test -p proxy-api --lib`
- [ ] `cargo test -p proxy-mcp --lib`
- [ ] `cargo test --workspace --all-targets`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] If deployed, verify via HTTP/MCP only; do not SSH to the dev address.

## Risk Points

- Keep dry-run side effects small; it should not open target tunnels.
- Avoid exposing sensitive proxy credentials in route decisions or logs.
- Keep protocol responses correct: HTTP 200 / SOCKS success only after an upstream tunnel is actually established.
- Do not include `.codex/config.toml` in task commits.
