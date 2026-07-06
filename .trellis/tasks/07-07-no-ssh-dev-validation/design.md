# Design: No SSH Dev Validation

## Scope

This task is intentionally a workflow hardening task. It changes documentation and test helper behavior so future release validation uses only public HTTP/MCP surfaces and GitHub Actions.

## Allowed Validation Surfaces

- GitHub Actions: proves the image for the pushed commit was built and pushed.
- MCP over HTTP: `update_service`, `service_status`, `pool_status`, and targeted smoke tools.
- REST API over HTTP: `/api/status`, `/api/healthz`, `/api/readyz`, `/api/metrics`, plus feature-specific endpoints.
- Integration tests: configured by `PROXY_POOL_HOST`, `PROXY_POOL_*_PORT`, and `PROXY_POOL_GIT_HASH`.

## Disallowed Surfaces

- Direct SSH to the dev address.
- External direct Docker API access to the host.
- Fault injection that mutates containers unless it is exposed as an explicit safe MCP/API operation.

## Helper Behavior

`tests/integration/helpers/docker_control.py` should not contain no-op destructive helpers. A no-op `stop_warp_container` makes tests look like they injected a failure when they did not. The safer behavior is to raise a dedicated exception that explains the missing safe control surface.

`clear_proxy_pool` stays API-only and can remain available because it uses public REST endpoints rather than host control.

## Documentation Shape

Add `docs/dev-validation.md` as the canonical checklist and link it from `CLAUDE.md`. Keeping the details in a standalone document avoids bloating project instructions while still making the rule discoverable.

## Rollback

These changes are non-destructive. Rollback is a normal git revert of documentation/helper edits.
