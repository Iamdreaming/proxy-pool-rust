# Implementation Plan: validator-observability-multitarget

## Steps

1. Read current task artifacts, package specs, and shared Trellis guides.
2. Add matrix request/result models and helpers in `proxy-core::validator`.
3. Add REST handler and route for `POST /api/proxy/check-matrix`.
4. Add MCP param type and `check_proxy_matrix` tool.
5. Update README, integration smoke expectations, Roadmap status, and Trellis specs if a reusable convention emerges.
6. Run focused tests first, then workspace test/clippy.
7. Archive the Trellis task and commit/push.

## Expected files

- `crates/proxy-core/src/validator.rs`
- `crates/proxy-api/src/routes.rs`
- `crates/proxy-mcp/src/lib.rs`
- `README.md`
- `tests/integration/test_l2_api.py`
- `tests/integration/test_l4_mcp.py`
- `docs/ROADMAP.md`
- `.trellis/tasks/07-07-validator-observability-multitarget/*`

## Verification

- `cargo fmt --all --check`
- `cargo test -p proxy-core`
- `cargo test -p proxy-api`
- `cargo test -p proxy-mcp`
- `cargo check -p proxy-server`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
