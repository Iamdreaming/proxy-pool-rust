# PRD: release-status-contract-smoke-v1

## Background

`release-validation-no-ssh-runbook-v2` made the default dev verification path
depend on a small set of read-only status fields. If those fields drift, the
runbook becomes unreliable even when the service still builds.

## Goal

Add minimal contract smoke coverage for the release validation status surfaces:
REST `/api/status`, MCP `service_status`, and MCP `update_status`. The tests
must prove the fields used by no-SSH validation remain present and
machine-readable without triggering `update_service`, Docker, Watchtower, or
SSH.

## Non-Goals

- Do not restore the full paused `mcp-api-contract-smoke-v2` task.
- Do not call `update_service` in tests.
- Do not require a live dev server, direct SSH, host Docker access, or
  Watchtower.
- Do not change update behavior unless a missing contract field requires a
  small serialization fix.
- Do not add Dashboard or release automation UI.

## Requirements

### F1: REST status release contract

- Add or tighten focused tests proving `/api/status` serialization includes:
  - top-level `version`
  - top-level `git_hash`
  - `release.git_hash`
  - `release.configured_image`
  - `release.update_enabled`
  - update container/image/watchtower URL metadata when configured
- Tests should work at the response/model serialization layer unless an
  existing lightweight route test harness already exists.

### F2: MCP service_status release contract

- Add focused MCP tests proving `service_status` serializes the same release
  metadata used by the runbook.
- Prefer shared core/API models if they already exist. Do not duplicate release
  metadata construction in adapters.

### F3: MCP update_status read-only contract

- Add tests proving `update_status` returns a readable shape before any update
  attempt, especially `status=never_triggered`.
- Cover at least one recorded status shape if the existing code exposes a safe
  in-memory helper for it.
- The tests must not call `update_service` or touch Docker/Watchtower.

### F4: Scope guard

- Keep this as the release-validation subset only. Broader REST/MCP parity for
  fetchers, subscriptions, routes, score explanations, or proxy checks remains
  paused under `mcp-api-contract-smoke-v2`.

## Acceptance Criteria

- [x] REST status tests cover the release fields required by
  `docs/dev-validation.md`.
- [x] MCP `service_status` tests cover the release fields required by
  `docs/dev-validation.md`.
- [x] MCP `update_status` tests cover the read-only `never_triggered` shape.
- [x] If feasible from existing helpers, tests cover one recorded update status
  shape without calling `update_service`.
- [x] Tests do not access SSH, host Docker, Watchtower, or a live dev server.
- [x] Docs/Roadmap reflect completion and the next TODO.
- [x] Focused tests pass locally; broader workspace checks are run if runtime
  code changes are made.

## Verification

- `cargo test -p proxy-api` if REST tests change.
- `cargo test -p proxy-mcp` if MCP tests change.
- `cargo test --workspace --all-targets` and `cargo clippy --workspace --all-targets -- -D warnings`
  if production code changes are needed.
- `rg` audit confirming tests do not call `update_service` except as an
  asserted non-default/mutating tool name.
