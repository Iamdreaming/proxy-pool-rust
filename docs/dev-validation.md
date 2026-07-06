# Dev Validation Without SSH

This project validates the dev deployment through public HTTP/MCP surfaces and
GitHub Actions. Do not SSH to the dev address for this workflow.

## Allowed Surfaces

- GitHub Actions for image build and push status.
- MCP over HTTP for `service_status`, `update_status`, `update_service`,
  `pool_status`, and feature-specific smoke tools such as `route_test` or
  `fetcher_status`.
- REST API over HTTP for `/api/status`, `/api/healthz`, `/api/readyz`, and
  `/api/metrics`.
- Integration tests configured through `PROXY_POOL_HOST`,
  `PROXY_POOL_API_PORT`, `PROXY_POOL_GW_PORT`, `PROXY_POOL_MCP_PORT`, and
  `PROXY_POOL_GIT_HASH`.

## Disallowed Surfaces

- Direct SSH to the dev address.
- Direct host Docker CLI or Docker API access from the test runner.
- Container fault injection unless a dedicated safe MCP/API operation exists.

The production compose file mounts `/var/run/docker.sock` into the service
container for internal MCP update and WARP optimizer behavior. That socket is
not a license for integration tests or agents to control the host directly.

Legacy/manual SSH-based scripts such as `deploy-remote.sh` are not part of this
dev validation workflow.

## Post-Push Checklist

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

3. Trigger the service update through MCP `update_service`.

   Before triggering, MCP `service_status` should expose
   `release.git_hash`, `release.configured_image`, `release.update_enabled`,
   and the configured update container. MCP `update_status` should show either
   `never_triggered` or the latest previous attempt.

   Use the configured MCP client/tooling for the dev instance. The expected
   success condition is that the service restarts onto the image built from the
   pushed commit. A dropped MCP response during container restart is acceptable
   only if the follow-up HTTP/MCP smoke checks prove the new service is healthy.

   After triggering, MCP `update_status` should distinguish:

   - `already_current`: the pulled image matches the running container image.
   - `updated`: Watchtower accepted the update request; verify the new git hash
     through HTTP after the restart.
   - `failed`: inspect the returned message and image identity fields, then fix
     configuration or retry through the public MCP surface.
   - `disabled`: update env wiring is not enabled for that environment.

4. Verify the deployed commit through HTTP.

   ```powershell
   $env:PROXY_POOL_HOST = "100.64.0.2"
   $env:PROXY_POOL_GIT_HASH = (git rev-parse --short HEAD)
   python -m pytest tests/integration/test_l1_health.py -q
   ```

   `/api/status` should include both the top-level `git_hash` and
   `release.git_hash`. The `release` object also reports the configured update
   image and container without requiring Docker socket access from the test
   runner.

5. Verify MCP and feature smoke paths.

   ```powershell
   python -m pytest tests/integration/test_l4_mcp.py -q
   ```

6. For a narrow feature, run the matching integration test file or an HTTP/MCP
   smoke command that checks the newly changed endpoint or tool.

## Fault Injection

Fault injection that mutates containers, routes, WARP instances, Watchtower, or
the Docker host is postponed until the project exposes an explicit safe MCP/API
operation for that scenario. If no such operation exists, mark the scenario as
manual/deferred and do not emulate it with SSH.
