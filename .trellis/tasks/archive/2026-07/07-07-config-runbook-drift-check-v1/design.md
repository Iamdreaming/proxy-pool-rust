# Design: config-runbook-drift-check-v1

## Boundaries

This is a documentation and L0 integration-test hardening task.

Likely touched files:

- `docs/dev-validation.md`
- `README.md` only if the short dev-validation pointer needs a small sync
- `tests/integration/test_l0_config_runbook_drift.py`
- `docs/ROADMAP.md` during closeout
- Trellis task artifacts

No runtime Rust behavior is planned. The code is already exposing
`ReleaseMetadata.configured_image`, `image_repo`, `image_tag`, and
`watchtower_url`.

## Drift Check Shape

Use a small pytest module that reads repository files as text and asserts the
operator contract stays aligned:

- `deploy/docker-compose.yml` contains the required app update env entries.
- `deploy/docker-compose.yml` wires Watchtower
  `WATCHTOWER_HTTP_API_TOKEN=${PROXY_POOL_UPDATE_TOKEN:-proxy-pool-update}`.
- `docs/dev-validation.md` lists the same env names and explains token
  matching.
- `docs/dev-validation.md` documents the release fields used by code/tests:
  `git_hash`, `configured_image`, `update_enabled`, `update_container`,
  `image_repo`, `image_tag`, and `watchtower_url`.
- Operator docs do not contain the obsolete `release.update_image` field.
- The runbook keeps the no-SSH, no host-Docker, and no routine
  `update_service` boundaries visible.
- The runbook records that Watchtower may lack common shell tools, so
  `docker compose exec watchtower-proxy-pool printenv` is not a recommended
  verification command.

The check intentionally uses explicit string assertions rather than adding a
YAML parser dependency. For this task, exact known deployment/runbook strings
are the contract.

## Compatibility

The drift test is local and read-only. It should work on Windows and Linux
because it only reads files under the repository root. It must not depend on
line endings, Docker Compose availability, or external services.

## Trade-Offs

Text assertions are narrower than semantic YAML parsing, but they keep the task
dependency-free and make the intended operator contract obvious. If the compose
structure changes substantially later, the failing test should force a human to
update both compose and docs together.

## Operational Notes

This task reinforces the existing boundary:

- Default validation uses GitHub Actions plus public HTTP/MCP read-only
  surfaces.
- `update_service` remains an explicit mutating update action.
- Direct SSH to the dev address remains disallowed for this workflow.
