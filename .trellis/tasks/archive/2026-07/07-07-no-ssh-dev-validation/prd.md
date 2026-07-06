# PRD: No SSH Dev Validation

## Background

The project can already build a GHCR image through GitHub Actions, trigger `update_service` through MCP, and verify runtime state through HTTP and MCP. The user explicitly disallowed direct SSH to the dev address, so deployment smoke tests and future fault-injection work must not rely on logging into the server.

## Goal

Make the dev validation path repeatable without direct SSH access, and remove misleading helper assumptions that suggest SSH or direct Docker API access is the default route.

## Confirmed Facts

- `CLAUDE.md` documents the post-fix deployment workflow: local tests, push, GitHub Actions, MCP `update_service`, then HTTP/MCP smoke checks.
- `tests/integration/config.py` points integration tests at the deployed instance through `PROXY_POOL_HOST`, API port, gateway port, and MCP port.
- `tests/integration/test_l1_health.py` verifies `/api/status`, `/api/healthz`, `/api/readyz`, port reachability, and expected `git_hash`.
- `tests/integration/test_l4_mcp.py` verifies MCP connectivity and the available tools, including `update_service`.
- `tests/integration/helpers/docker_control.py` still says fault-injection helpers require SSH or direct Docker API access, and its WARP control functions are TODO no-ops.
- `deploy/docker-compose.yml` intentionally mounts `/var/run/docker.sock` inside the service container for MCP `update_service` and WARP optimizer internals. This is container-internal capability, not permission for external SSH or direct host Docker control.

## Requirements

### F1: No-SSH Dev Workflow Documentation

- Add a reusable dev validation document that states SSH to the dev address is forbidden for this workflow.
- The document must list the allowed validation surfaces:
  - GitHub Actions build result
  - MCP `update_service`
  - REST `/api/status`, `/api/healthz`, `/api/readyz`, `/api/metrics`
  - MCP smoke tools such as `service_status`, `pool_status`, `route_test`, and `fetcher_status`
  - integration tests configured through public host/port environment variables
- The document must include a minimal command checklist for post-push smoke validation.
- The document must explain that destructive or fault-injection checks are postponed unless exposed through an explicit safe MCP/API operation.

### F2: Project Workflow Alignment

- Update the root project workflow notes so future agents do not infer that SSH is allowed during dev validation.
- Existing local test, GitHub Actions, MCP update, and HTTP/MCP verification steps should remain intact.

### F3: Safe Integration Helper Behavior

- Update `tests/integration/helpers/docker_control.py` so it no longer advertises SSH or direct Docker API as normal integration-test paths.
- WARP fault-injection helpers must fail loudly with a clear "unavailable without explicit safe control surface" error instead of silently passing.
- Non-destructive HTTP helpers such as `clear_proxy_pool` may remain API-only.

### F4: Verification

- Run Python syntax verification for the affected integration helpers.
- If feasible, run a lightweight unit-style check that the fault-injection helper rejects unsafe use.
- Do not SSH to the dev address during validation.

## Non-Goals

- No direct SSH usage.
- No new remote Docker API tunnel.
- No destructive fault injection against dev.
- No changes to `update_service` internals.
- No attempt to resume or archive paused `gateway-route-debugging` or `fetcher-validator-quality`.

## Acceptance Criteria

1. Documentation gives a complete no-SSH dev validation checklist.
2. Root workflow instructions explicitly state the no-SSH rule and point to the checklist.
3. `docker_control.py` no longer describes SSH/direct Docker access as the default path.
4. WARP fault-injection helper calls fail loudly instead of silently doing nothing.
5. `python -m py_compile` passes for affected Python integration files.
6. No SSH command is executed as part of this task.
