# PRD: dev-update-config-doc-hardening-v1

## Background

The dev deployment already uses GHCR images, `proxy-pool` self-update
environment variables, and a Watchtower sidecar. The no-SSH runbook documents
the main update env contract, but it still leaves some operator details spread
across compose, README, and memory: which container owns which responsibility,
why the labels differ, how Watchtower HTTP update is expected to be enabled,
and what a safe rollback/pause decision looks like without turning the runbook
into an automatic host-Docker procedure.

## Goal

Make the dev self-update wiring understandable and checkable from repository
docs, while preserving the no-SSH and read-only-by-default validation boundary.

## Requirements

- Document `proxy-pool`, `watchtower-proxy-pool`, and `redis` roles for the
  managed dev compose deployment.
- Document the key `proxy-pool` update env vars and explain that dev is wired
  for `PROXY_POOL_UPDATE_ENABLED=true`, GHCR `latest`, Docker socket access
  inside the service, and Watchtower HTTP API update.
- Document Watchtower's `--http-api-update --cleanup --label-enable` command and
  label behavior:
  - `proxy-pool` is update-eligible with
    `com.centurylinklabs.watchtower.enable=true`.
  - `watchtower-proxy-pool` is intentionally not self-updated by the same
    Watchtower instance.
- Document no-SSH verification paths through compose source, `/api/status`,
  MCP `service_status`, MCP `update_status`, and GitHub Actions.
- Document rollback/pause thinking as an explicit operator decision, not an
  automatic smoke-runner action.
- Extend the local drift guard so the new documentation does not silently drift.

## Acceptance Criteria

- [x] `docs/dev-validation.md` contains a managed dev compose roles section.
- [x] `docs/dev-validation.md` explains Watchtower command, labels, and token
      pairing.
- [x] `docs/dev-validation.md` states dev update is configured for
      `PROXY_POOL_UPDATE_ENABLED=true`, GHCR latest, and Watchtower HTTP API.
- [x] `docs/dev-validation.md` includes safe rollback/pause guidance without
      instructing agents/tests to SSH or use host Docker as the default path.
- [x] README's dev validation summary points operators to the runbook for
      compose self-update wiring.
- [x] `tests/integration/test_l0_config_runbook_drift.py` asserts the new
      container role, label, Watchtower command, and rollback/pause wording.
- [x] Local drift tests pass.

## Out of Scope

- No compose behavior changes.
- No automatic rollback implementation.
- No host Docker, SSH, or fault-injection validation.
- No production deployment changes.
