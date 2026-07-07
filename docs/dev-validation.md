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

### Shortcut

The preferred shortcut is the read-only smoke runner:

```powershell
$env:PROXY_POOL_HOST = "100.64.0.2"
$env:PROXY_POOL_GIT_HASH = (git rev-parse --short HEAD)
python tests\integration\readonly_dev_smoke.py --branch main --wait-ci
```

Use `--skip-ci` when you only want HTTP/MCP public-surface checks, for example
while iterating locally or when GitHub CLI authentication is unavailable.

The runner checks GitHub Actions, `/api/status`, `/api/readyz`, MCP
`service_status`, and MCP `update_status`. It does not SSH, access host Docker,
call `update_service`, refresh the pool, delete proxies, or apply any remote
mutation.

For business availability checks after the basic release smoke is healthy, run:

```powershell
python tests\integration\business_e2e_smoke.py --json
```

This checks the public gateway and stored proxy candidates against business
targets such as Cloudflare trace, GitHub, OpenAI API, and Reddit. It is also
observational only: it reads `/api/proxies/scores` and calls the diagnostic
`/api/proxy/check-matrix` endpoint, but it does not refresh, delete, update, or
apply changes.

The manual steps below remain useful for drilling into a failed runner result.

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
   `release.update_container`, `release.image_repo`,
   `release.image_tag`, and `release.watchtower_url`.

   MCP `update_status` should report the latest in-process update snapshot
   without touching Docker or Watchtower. Common statuses are
   `never_triggered`, `disabled`, `already_current`, `updated`, and `failed`.

6. Verify MCP and feature smoke paths.

   ```powershell
   python -m pytest tests/integration/test_l4_mcp.py -q
   ```

7. For a narrow feature, run the matching integration test file or an HTTP/MCP
   smoke command that checks the newly changed endpoint or tool.

## Managed Dev Compose Roles

The managed dev compose file has three service roles:

| Service | Role |
|---------|------|
| `redis` | Persistent proxy pool state and scheduler/status backing store. |
| `proxy-pool` | Main application container. It exposes the gateway, REST API, MCP HTTP transport, reads `/app/config/settings.yaml`, and owns the internal MCP `update_service` operation when explicitly chosen. |
| `watchtower-proxy-pool` | Watchtower sidecar. It exposes Watchtower's HTTP update API inside the compose network, pulls eligible labeled containers, and removes old images with cleanup enabled. |

The `proxy-pool` container is update-eligible because compose labels it with
`com.centurylinklabs.watchtower.enable=true`. The Watchtower sidecar is labeled
`com.centurylinklabs.watchtower.enable=false` so the same Watchtower instance
does not try to update itself.

Watchtower is started with:

```text
--http-api-update --cleanup --label-enable
```

In short, the managed Watchtower command is
`--http-api-update --cleanup --label-enable`.

That means dev updates are intentionally label-scoped and HTTP-triggered:
`update_service` pulls the configured image through the Docker socket available
inside `proxy-pool`, then calls the Watchtower HTTP API at
`http://watchtower-proxy-pool:8080/v1/update`. The HTTP API token in Watchtower
must match the token used by `proxy-pool`.

## Dev Update Environment Expectations

Managed dev compose wiring should keep these settings aligned:

- `PROXY_POOL_UPDATE_ENABLED=true`
- `PROXY_POOL_UPDATE_DOCKER_SOCKET=/var/run/docker.sock`
- `PROXY_POOL_UPDATE_CONTAINER=proxy-pool`
- `PROXY_POOL_UPDATE_IMAGE=ghcr.io/iamdreaming/proxy-pool-rust:latest`
- `PROXY_POOL_UPDATE_WATCHTOWER_URL=http://watchtower-proxy-pool:8080/v1/update`
- `PROXY_POOL_UPDATE_TOKEN` in the app container matches
  `WATCHTOWER_HTTP_API_TOKEN` in the Watchtower container.

With this wiring, managed dev is already configured for explicit MCP
`update_service` updates from `ghcr.io/iamdreaming/proxy-pool-rust:latest`.
GitHub Actions owns publishing that `latest` image; the running service and
MCP status surfaces own proving which git hash is currently live.

Verify these through deployment configuration, `/api/status.release`, MCP
`service_status.release`, or MCP `update_status`. Do not SSH to the host just
to print environment variables.

The `containrrr/watchtower` image may not include common shell utilities such
as `printenv`, so `docker compose exec watchtower-proxy-pool printenv` is not a
recommended token check. Prefer the compose file, app-container update env,
`service_status.release`, and `update_status` instead.

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

## Rollback And Pause Guidance

Rollback or update pause is an explicit operator decision. The default smoke
runner and integration tests must only report public state; they must not
change image tags, disable updates, stop containers, or call host Docker.

Safe decision points:

- To pause automatic eligibility, change the managed deployment configuration
  so `PROXY_POOL_UPDATE_ENABLED=false` or the Watchtower enable label no longer
  selects `proxy-pool`, then redeploy through the approved operator path.
- To roll back, pin `PROXY_POOL_UPDATE_IMAGE` or the compose image to a known
  good tag/digest, then perform an explicit update through the approved
  operator path.
- After any rollback or pause, verify only through GitHub Actions history,
  `/api/status.release`, `/api/readyz`, MCP `service_status`, and MCP
  `update_status` unless the operator has intentionally chosen a separate
  mutating maintenance step.

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
