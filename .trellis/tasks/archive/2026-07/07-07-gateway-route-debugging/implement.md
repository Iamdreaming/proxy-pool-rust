# Implementation Plan: Gateway Route Debugging

## Phase 1: Planning

- [x] Update roadmap to make `gateway-route-debugging` the active task.
- [x] Isolate paused `fetcher-validator-quality` WIP in a git stash.
- [x] Create Trellis task.
- [x] Inspect current router, gateway handlers, API routes, MCP tools, metrics, and integration tests.
- [x] Write PRD, design, and implementation plan.

## Phase 2: Route Decision Model

- [x] Add route decision and candidate structs with serde support.
- [x] Add stable exit/status enums.
- [x] Add tests for serialization and representative candidate orders.

## Phase 3: Traceable Selector

- [x] Add `select_with_trace(host, protocol)` while preserving `select(host, protocol)`.
- [x] Record matched route group and matched reason where current router data allows it.
- [x] Record GeoIP summary when GeoIP is consulted.
- [x] Record unavailable reasons for pool, WARP, xray, and no-proxy cases.
- [x] Add selector tests for explicit direct, explicit free_pool, overseas fallback, and no-upstream.

## Phase 4: Gateway Runtime Attempts

- [x] Update HTTP CONNECT handler to use traceable selection.
- [x] Update SOCKS5 handler to use traceable selection.
- [x] Add structured attempt logging.
- [x] Implement retry-across-candidates before client success response if feasible within current handler structure.
- [x] Add gateway tests for success/failure and retry behavior if runtime retry is implemented.

## Phase 5: API And MCP

- [x] Add route dry-run endpoint to `proxy-api`.
- [x] Add `route_test` tool to `proxy-mcp`.
- [x] Ensure API/MCP share selector logic instead of duplicating route decisions.
- [x] Add serialization/deserialization tests.

## Phase 6: Metrics

- [x] Add gateway route outcome metrics with stable names and labels.
- [x] Render metrics through `/api/metrics`.
- [x] Add metrics unit tests.

## Phase 7: Verification

- [x] `cargo fmt --all --check`
- [x] `cargo test -p proxy-core --lib`
- [x] `cargo test -p proxy-gateway --lib`
- [x] `cargo test -p proxy-api --lib`
- [x] `cargo test -p proxy-mcp --lib`
- [x] `cargo test --workspace --all-targets`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] If deployed, verify via HTTP/MCP only; do not SSH to the dev address.
- [x] Record closeout verification in `verification.md`.

## Risk Points

- Keep dry-run side effects small; it should not open target tunnels.
- Avoid exposing sensitive proxy credentials in route decisions or logs.
- Keep protocol responses correct: HTTP 200 / SOCKS success only after an upstream tunnel is actually established.
- Do not include `.codex/config.toml` in task commits.
