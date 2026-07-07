# Readonly dev smoke runner v1

## Background

The project already has a no-SSH dev validation policy in `docs/dev-validation.md`.
The allowed surfaces are GitHub Actions, public REST status endpoints, MCP
read-only tools, and existing integration tests. The current workflow still
requires operators to manually stitch together several commands after each push.

Current evidence:

- `.github/workflows/docker-build.yml` builds and pushes GHCR images for `main`.
- `docs/dev-validation.md` defines the post-push checklist and forbids direct
  SSH, host Docker access, and routine `update_service` calls.
- `tests/integration/test_l1_health.py` already verifies HTTP status, readiness,
  open ports, and optional runtime git hash matching through `PROXY_POOL_*`
  environment variables.
- `tests/integration/test_l4_mcp.py` already verifies MCP `service_status` and
  read-only `update_status` shape.
- `tests/integration/helpers/mcp_client.py` provides a reusable Streamable HTTP
  MCP client.

## Goal

Add a local, repeatable, read-only dev smoke runner that summarizes whether the
current `main` push has a successful image build and whether the dev public
surfaces report the expected runtime status.

## User Value

After a push, the operator can run one command and get a concise answer:

- Did the latest GitHub Actions Docker build finish successfully?
- Is the public REST API reachable and ready?
- Does runtime `git_hash` match the expected commit when an expected hash is
  provided?
- Do MCP `service_status` and `update_status` return usable read-only release
  state?
- If something fails, which public surface should be checked next?

## Requirements

### R1: Read-only execution

The runner must not mutate the dev deployment.

- It must not SSH to the dev address.
- It must not call host Docker CLI or host Docker API.
- It must not call MCP `update_service`.
- It must not trigger refresh, cleanup, delete, or any apply-style operation.

### R2: Local command entry point

Provide a single local command that can be run from the repository root.

Preferred implementation: a Python script under `tests/integration/` or a
nearby helper path that reuses the existing integration-test dependencies and
environment variables:

- `PROXY_POOL_HOST`
- `PROXY_POOL_API_PORT`
- `PROXY_POOL_GW_PORT`
- `PROXY_POOL_MCP_PORT`
- `PROXY_POOL_GIT_HASH`

### R3: GitHub Actions check

When the GitHub CLI is available, the runner should inspect the latest
`docker-build.yml` run for the selected branch, defaulting to `main`.

The runner should report:

- workflow run id
- status/conclusion
- head SHA or short SHA when available
- URL or command hint for log inspection when failed

The runner may support a flag to skip CI checks for offline/local use.

### R4: HTTP status checks

The runner should query public HTTP endpoints only:

- `GET /api/status`
- `GET /api/readyz`

The summary should include:

- runtime `git_hash`
- `release.git_hash`
- release image metadata when present
- readiness status and dependency message when unhealthy

### R5: MCP read-only checks

The runner should call only read-only MCP tools:

- `service_status`
- `update_status`

The summary should include:

- MCP release `git_hash`
- update enabled flag and configured image/container when present
- latest update status: `never_triggered`, `disabled`, `already_current`,
  `updated`, or `failed`

### R6: Failure behavior

The runner should return exit code `0` only when all enabled checks pass.
Any failed enabled check should return a non-zero exit code and print a compact
failure list with the next public-surface triage hint.

### R7: Documentation

Document the command in `docs/dev-validation.md` and reference it from README if
the command becomes the preferred post-push shortcut.

## Acceptance Criteria

- [x] A Trellis-reviewed implementation plan exists before coding starts.
- [x] A single local command runs the read-only smoke runner from the repo root.
- [x] The command can check latest `docker-build.yml` status through `gh` when
  available, and can skip CI checks when requested.
- [x] The command checks `/api/status` and `/api/readyz` using only public HTTP.
- [x] The command checks MCP `service_status` and `update_status` without
  calling `update_service`.
- [x] The command supports the same `PROXY_POOL_*` environment variables already
  used by integration tests.
- [x] Runtime git hash mismatch against `PROXY_POOL_GIT_HASH` fails clearly.
- [x] Any failed enabled check exits non-zero and prints a useful triage hint.
- [x] The runner itself has local tests or a py_compile/import-level validation
  that does not require live dev.
- [x] `docs/dev-validation.md` documents the new command and keeps the no-SSH
  boundary explicit.

## Out of Scope

- Triggering `update_service` or any other mutating MCP tool.
- SSH, host Docker CLI/API access, or container fault injection.
- Full REST/MCP contract coverage. That remains separate from the paused
  `mcp-api-contract-smoke-v2` task.
- Building a web UI or dashboard for the smoke result.
- Replacing the existing integration test suite.

## Decision

- Chosen: Python command under `tests/integration/` that reuses existing config
  and MCP helper code, with flags for CI wait/skip and output format.
- Rejected: thin pytest wrapper around existing `test_l1_health.py` and
  `test_l4_mcp.py`; lower code cost, weaker summary and CI integration.
- Rejected: PowerShell script; convenient on the current workstation, less
  portable for CI and other operators.
