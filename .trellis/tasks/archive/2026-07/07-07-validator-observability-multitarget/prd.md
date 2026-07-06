# PRD: validator-observability-multitarget

## Background

`check_proxy` already returns a structured single-target diagnostic result with target URL, target host, HTTP status, timings, observed exit IP/country, and stable error categories. Operators still need to know whether a proxy fails globally or only against specific destinations. That requires a small validation matrix instead of repeatedly calling `check_proxy` by hand.

## Goal

Add a multi-target proxy validation matrix that reuses `proxy-core::validator` diagnostics and exposes the result through REST and MCP without direct dev SSH.

## Non-goals

- Do not change the existing background validation loop or proxy retention policy.
- Do not add persistent storage for historical validation results in this slice.
- Do not introduce Web Dashboard UI changes in this slice.
- Do not perform destructive dev fault injection or modify dev compose.

## Requirements

### F1: Core validation matrix

- Define a reusable request/result model in `proxy-core`.
- Accept a proxy host, port, protocol, optional timeout, and optional target URLs.
- If no target URLs are provided, use a safe default matrix:
  - `https://www.cloudflare.com/cdn-cgi/trace`
  - `https://httpbin.org/ip`
- Execute each target through the existing `Validator::check_one()` path.
- Return one `ProxyCheckResult` per target and a summary with total/alive/failed counts.

### F2: REST API

- Add `POST /api/proxy/check-matrix`.
- Request body supplies proxy fields and optional `targets` / `timeout_secs`.
- Response body is the core matrix result, not a separately reassembled API-only shape.
- Invalid or empty target URLs should be rejected with HTTP 400 and a structured status message.

### F3: MCP tool

- Add MCP tool `check_proxy_matrix`.
- Keep existing `check_proxy` behavior unchanged.
- Tool parameters mirror REST as closely as practical.
- Response serializes the same core matrix result used by REST.

### F4: Docs and tests

- Add unit tests for default targets, request validation, and result serialization.
- Add API/MCP smoke assertions or contract tests where practical.
- Update README and Roadmap/Trellis state.

## Acceptance Criteria

- [x] `proxy-core` exposes a reusable matrix function/model that returns per-target `ProxyCheckResult`.
- [x] REST `POST /api/proxy/check-matrix` returns summary and per-target checks.
- [x] MCP `check_proxy_matrix` returns the same core result shape.
- [x] Existing `check_proxy` single-target behavior remains compatible.
- [x] Invalid matrix requests fail fast without network calls.
- [x] Relevant Rust tests pass.
- [x] No direct SSH is used for validation.

## Validation

- `cargo fmt --all --check`
- `cargo test -p proxy-core`
- `cargo test -p proxy-api`
- `cargo test -p proxy-mcp`
- `cargo check -p proxy-server`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
