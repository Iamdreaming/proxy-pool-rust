# PRD: release-validation-no-ssh-runbook-v2

## Background

The project already has no-SSH guidance and release/update status fields, and
the dev compose environment has been confirmed to expose the expected
`PROXY_POOL_UPDATE_*` and Watchtower token settings. The remaining operational
gap is that post-push verification is still easy to reconstruct from memory
instead of following one small, repeatable checklist.

## Goal

Document a repeatable post-push dev validation path using GitHub Actions,
public HTTP status, and read-only MCP status tools. The runbook should let an
operator determine whether dev is running the expected image and git hash
without directly SSHing to the dev host and without defaulting to
`update_service`.

## Non-Goals

- Do not change runtime update behavior.
- Do not trigger `update_service` as part of this task.
- Do not add full REST/MCP contract smoke coverage; that remains paused under
  `mcp-api-contract-smoke-v2`.
- Do not require direct SSH, host Docker access, or server shell commands.
- Do not restore paused `fetcher-source-quality-ranking`, Dashboard, WARP,
  xray, recommendation, or update-failure WIP.

## Requirements

### F1: Single no-SSH validation checklist

- Document the exact post-push order:
  1. Confirm GitHub Actions image build/push status.
  2. Check public HTTP status/readiness.
  3. Check read-only MCP status surfaces when available.
  4. Compare expected git hash/image metadata with reported runtime metadata.
- Keep the checklist short enough to use during every release.

### F2: Allowed and forbidden operations

- Explicitly list allowed verification channels: GitHub Actions, public HTTP
  status endpoints, MCP `service_status`, and MCP `update_status`.
- Explicitly list forbidden default actions: direct SSH to dev address, host
  Docker shell/API access, and calling `update_service` unless the operator
  intentionally chooses an update step outside this runbook.

### F3: Environment expectations

- Record the minimum dev compose expectations that matter for release
  verification:
  - `PROXY_POOL_UPDATE_ENABLED=true`
  - update container/image names
  - Watchtower HTTP URL
  - token matching between app and Watchtower
- Make clear these values should be verified through deployment config or
  status surfaces, not by directly SSHing to the host.

### F4: Failure triage table

- Document common failure branches and next checks:
  - CI build failed or is still running.
  - GHCR image pushed but runtime git hash is old.
  - status endpoint is unavailable.
  - release metadata is missing or inconsistent.
  - `update_status` reports disabled, never triggered, already current, updated,
    or failed.

### F5: Documentation-only delivery

- Prefer updating `docs/dev-validation.md` and, if useful, the README release
  section.
- Keep this task focused on runbook clarity; code/test changes belong to
  `release-status-contract-smoke-v1`.

## Acceptance Criteria

- [x] The no-SSH post-push checklist exists in project documentation.
- [x] The checklist names the exact HTTP and MCP status surfaces to use.
- [x] The checklist includes a compact expected-vs-observed git hash/image
  comparison step.
- [x] The docs explicitly prohibit direct SSH as the default dev validation
  path.
- [x] The docs explicitly state that `update_service` is not part of the
  default read-only validation checklist.
- [x] The docs capture the dev compose environment expectations needed for
  update visibility.
- [x] The docs include failure triage for CI, image, runtime status, and update
  status mismatches.
- [x] Roadmap is updated when the task is completed or paused.

## Verification

- Review the changed docs for consistency with current Roadmap no-SSH policy.
- Run a text search to confirm no new instructions tell operators to SSH into
  dev for validation.
- No runtime tests are required unless code changes are introduced.
