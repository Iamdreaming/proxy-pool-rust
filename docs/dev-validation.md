# Dev Validation Without SSH

This project validates the dev deployment through public HTTP/MCP surfaces and
GitHub Actions. Do not SSH to the dev address for this workflow.

## Allowed Surfaces

- GitHub Actions for image build and push status.
- MCP over HTTP for read-only `service_status`, `update_status`, `pool_status`,
  and feature-specific smoke tools such as `route_test` or `fetcher_status`.
- REST API over HTTP for `/api/status`, `/api/healthz`, `/api/readyz`, and
  `/api/metrics`.
- Integration tests configured through `PROXY_POOL_HOST`,
  `PROXY_POOL_API_PORT`, `PROXY_POOL_GW_PORT`, `PROXY_POOL_MCP_PORT`, and
  `PROXY_POOL_GIT_HASH`.
- Explicitly chosen MCP `update_service` calls when the operator intends to
  mutate the dev deployment. This is not part of the default read-only
  validation checklist.

## Disallowed Surfaces

- Direct SSH to the dev address.
- Direct host Docker CLI or Docker API access from the test runner.
- Calling `update_service` as a routine status check.
- Container fault injection unless a dedicated safe MCP/API operation exists.

The production compose file mounts `/var/run/docker.sock` into the service
container for internal MCP update and WARP optimizer behavior. That socket is
not a license for integration tests or agents to control the host directly.

Legacy/manual SSH-based scripts such as `deploy-remote.sh` are not part of this
dev validation workflow.

## Post-Push Checklist

This default checklist is read-only after the push. It tells you whether dev is
already running the expected image/git hash. If it is not, decide explicitly
whether to trigger an update; do not treat `update_service` as a status check.

1. Run local checks relevant to the change.

   ```powershell
   cargo test --workspace --all-targets
   cargo clippy --workspace --all-targets -- -D warnings
   ```

2. Push the commit and wait for the Docker build workflow.

   ```powershell
   git push origin main
   gh run list --workflow=docker-build.yml --branch main --limit 1
   gh run watch <run-id> --exit-status
   ```

3. Record the expected runtime identity.

   ```powershell
   $env:PROXY_POOL_GIT_HASH = (git rev-parse --short HEAD)
   ```

   The Docker workflow builds the GHCR image from this commit. The follow-up
   checks compare this expected short hash with the runtime status surfaces.

4. Check public HTTP status and readiness.

   Use the dev HTTP target only as an HTTP endpoint, not as an SSH target.
   `/api/status` should include both the top-level `git_hash` and
   `release.git_hash`. `/api/readyz` should report dependency readiness.

   ```powershell
   $env:PROXY_POOL_HOST = "100.64.0.2"
   python -m pytest tests/integration/test_l1_health.py -q
   ```

5. Check read-only MCP release status when MCP transport is available.

   MCP `service_status` should expose `release.git_hash`,
   `release.configured_image`, `release.update_enabled`,
   `release.update_container`, `release.update_image`, and
   `release.watchtower_url`.

   MCP `update_status` should report the latest in-process update snapshot
   without touching Docker or Watchtower. Common statuses are
   `never_triggered`, `disabled`, `already_current`, `updated`, and `failed`.

6. Verify MCP and feature smoke paths.

   ```powershell
   python -m pytest tests/integration/test_l4_mcp.py -q
   ```

7. For a narrow feature, run the matching integration test file or an HTTP/MCP
   smoke command that checks the newly changed endpoint or tool.

## Dev Update Environment Expectations

Managed dev compose wiring should keep these settings aligned:

- `PROXY_POOL_UPDATE_ENABLED=true`
- `PROXY_POOL_UPDATE_CONTAINER=proxy-pool`
- `PROXY_POOL_UPDATE_IMAGE=ghcr.io/iamdreaming/proxy-pool-rust:latest`
- `PROXY_POOL_UPDATE_WATCHTOWER_URL=http://watchtower-proxy-pool:8080/v1/update`
- `PROXY_POOL_UPDATE_TOKEN` in the app container matches
  `WATCHTOWER_HTTP_API_TOKEN` in the Watchtower container.

Verify these through deployment configuration, `/api/status.release`, MCP
`service_status.release`, or MCP `update_status`. Do not SSH to the host just
to print environment variables.

## Optional Explicit Update Step

If the read-only checklist proves that dev is still running an old image/git
hash, the operator may explicitly choose to call MCP `update_service`. Treat
that as a mutating deployment action, not as validation.

Before calling it, confirm through `service_status` that updates are enabled and
the configured image/container/watchtower URL are expected. After calling it,
poll `/api/status.git_hash` or rerun the HTTP health smoke until the runtime
matches the expected short hash.

A dropped MCP response during container restart is acceptable only if the
follow-up HTTP/MCP smoke checks prove the new service is healthy.

## Failure Triage

| Symptom | Next check |
|---------|------------|
| GitHub Actions is still running | Wait for `gh run watch <run-id> --exit-status`; do not inspect the host. |
| GitHub Actions failed | Use `gh run view <run-id> --log-failed`; fix and push again. |
| Image build succeeded but `/api/status.git_hash` is old | Check read-only MCP `service_status.release` and `update_status`; decide whether an explicit update is needed. |
| `/api/status` or `/api/readyz` is unavailable | Treat dev as unhealthy from public surfaces; do not SSH as the default next step. |
| `release` metadata is missing or inconsistent | Record the response shape and handle it under `release-status-contract-smoke-v1`. |
| `update_status=never_triggered` | No update has been recorded in this process; compare runtime git hash before deciding whether to update. |
| `update_status=disabled` | Update env wiring is disabled or missing for that environment. |
| `update_status=already_current` | The pulled image matched the running image; verify the expected git hash through HTTP. |
| `update_status=updated` | Watchtower accepted the update; verify the new git hash through HTTP after restart. |
| `update_status=failed` | Inspect the structured message and image identity fields, then fix config or retry through the public MCP surface only when intentional. |

## Fault Injection

Fault injection that mutates containers, routes, WARP instances, Watchtower, or
the Docker host is postponed until the project exposes an explicit safe MCP/API
operation for that scenario. If no such operation exists, mark the scenario as
manual/deferred and do not emulate it with SSH.
