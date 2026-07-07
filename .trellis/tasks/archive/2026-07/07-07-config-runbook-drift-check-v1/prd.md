# PRD: config-runbook-drift-check-v1

## Goal

Prevent drift between the dev deployment runbook, README guidance,
`deploy/docker-compose.yml`, and the read-only release/update status contracts
used by the no-SSH dev validation workflow.

The delivered slice should make the current dev update wiring and public
read-only validation contract easy to verify locally, without SSH, host Docker
access, or routine `update_service` calls.

## Background And Evidence

- Roadmap lists this task as the next P0 item to prevent README,
  `docs/dev-validation.md`, dev compose/env examples, and status field names
  from drifting.
- `deploy/docker-compose.yml:34` through `deploy/docker-compose.yml:39`
  configures the app container with `PROXY_POOL_UPDATE_ENABLED`,
  `PROXY_POOL_UPDATE_DOCKER_SOCKET`, `PROXY_POOL_UPDATE_CONTAINER`,
  `PROXY_POOL_UPDATE_IMAGE`, `PROXY_POOL_UPDATE_WATCHTOWER_URL`, and
  `PROXY_POOL_UPDATE_TOKEN`.
- `deploy/docker-compose.yml:67` wires Watchtower's
  `WATCHTOWER_HTTP_API_TOKEN` to the same `PROXY_POOL_UPDATE_TOKEN` default.
- `docs/dev-validation.md:97` through `docs/dev-validation.md:100` currently
  documents `release.update_image`, but the code and tests use
  `release.configured_image`.
- `crates/proxy-core/src/status.rs:34` defines
  `ReleaseMetadata.configured_image`; `crates/proxy-core/src/status.rs:52`
  through `crates/proxy-core/src/status.rs:66` derives image repo/tag and
  watchtower metadata from the `PROXY_POOL_UPDATE_*` environment.
- `crates/proxy-mcp/src/lib.rs:170` through `crates/proxy-mcp/src/lib.rs:189`
  reads the same update env set for MCP `update_service` / `update_status`.
- `tests/integration/readonly_dev_smoke.py:214` and
  `tests/integration/test_l0_readonly_dev_smoke.py:72` already expect
  `release.configured_image`.
- User-provided dev output confirmed the app container update env values match
  compose, and also confirmed the Watchtower image may not include `printenv`.

## Requirements

### R1: Canonical dev update wiring

The runbook must list the canonical dev update env wiring from compose:

- `PROXY_POOL_UPDATE_ENABLED=true`
- `PROXY_POOL_UPDATE_DOCKER_SOCKET=/var/run/docker.sock`
- `PROXY_POOL_UPDATE_CONTAINER=proxy-pool`
- `PROXY_POOL_UPDATE_IMAGE=ghcr.io/iamdreaming/proxy-pool-rust:latest`
- `PROXY_POOL_UPDATE_WATCHTOWER_URL=http://watchtower-proxy-pool:8080/v1/update`
- `PROXY_POOL_UPDATE_TOKEN` matching `WATCHTOWER_HTTP_API_TOKEN`

It must explain that the Docker socket is mounted for internal MCP update and
WARP optimizer behavior, not as permission for tests or agents to control the
host directly.

### R2: Correct release/status field names

Docs and README-visible guidance must use the actual release field contract:

- `release.git_hash`
- `release.configured_image`
- `release.update_enabled`
- `release.update_container`
- `release.image_repo`
- `release.image_tag`
- `release.watchtower_url`

The obsolete `release.update_image` name must not appear in operator docs.

### R3: Watchtower shell-tool limitation

The runbook must record that `containrrr/watchtower` may not include common
shell utilities such as `printenv`. The recommended verification path should
not rely on `docker compose exec watchtower-proxy-pool printenv`.

### R4: Local drift check

Add a lightweight local test/check that fails when the docs, compose file, or
status contract drift on the items above. The check must be read-only and must
not require Docker, SSH, network, GitHub authentication, Redis, or a running
proxy-pool service.

### R5: Preserve no-SSH / no-mutation boundaries

The task must not call `update_service`, update dev, access the host Docker
API, or SSH into the dev machine. It may read local repository files and run
local tests only.

## Acceptance Criteria

- [x] `docs/dev-validation.md` documents the canonical compose update envs,
      actual release field names, and Watchtower `printenv` limitation.
- [x] Operator docs no longer describe `release.update_image`; they describe
      `release.configured_image` and image repo/tag instead.
- [x] A local L0 drift test covers compose env wiring, Watchtower token wiring,
      documented release fields, and the no-SSH/no-routine-update boundary.
- [x] The new test does not call live HTTP/MCP, Docker, SSH, or
      `update_service`.
- [x] Focused Python compile/test validation passes for the new drift check
      and adjacent no-SSH/read-only smoke tests.

## Out Of Scope

- Mutating dev through `update_service`.
- Fixing the currently deployed dev runtime if it is behind `main`.
- Full REST/MCP contract-smoke expansion.
- Update failure injection or Watchtower failure hardening.
- Changing runtime Rust status/update behavior unless local evidence shows the
  docs cannot be aligned without it.
