# PRD: release-status-public-smoke-v1

## Background

The project already has a full read-only post-push runner and broader
integration suites. Operators still need a very small public-surface smoke that
can be run locally to guard the exact release status fields used by no-SSH dev
validation without re-opening the paused full REST/MCP contract smoke effort.

## Goal

Add lightweight public read-only release status smoke coverage for the fields
that prove what build is running and whether the service is ready.

## Requirements

- Add a focused integration smoke for `GET /api/status`.
- Add a focused integration smoke for `GET /api/readyz`.
- Add focused MCP read-only smoke for `service_status` and `update_status`.
- Reuse `tests/integration/config.py` and `tests/integration/helpers/mcp_client.py`.
- Do not call MCP `update_service` or any refresh/apply/delete/cleanup tool.
- Do not use direct SSH, host Docker CLI/API, or private host state.
- Keep the smoke narrower than `test_l2_api.py` and `test_l4_mcp.py`; it should
  only protect the public release validation contract.

## Acceptance Criteria

- [x] New smoke checks `/api/status` release, quality, Redis, WARP, and Xray
      summary shapes.
- [x] New smoke checks `/api/readyz` allows HTTP 200 or 503 and validates the
      structured readiness body.
- [x] New smoke checks MCP `service_status` exposes the same release contract as
      `/api/status`.
- [x] New smoke checks MCP `update_status` returns a known status and required
      read-only metadata when an update attempt has been recorded.
- [x] Tests do not call `update_service` or other mutating MCP/API paths.
- [x] Local validation passes without requiring a live dev instance for helper
      logic where possible; live smoke can be run explicitly.

## Out Of Scope

- No full REST/MCP contract smoke restoration.
- No deployment update, Watchtower mutation, pool refresh, proxy cleanup, or
  config apply.
- No changes to runtime API/MCP response models unless an existing contract bug
  is discovered.
