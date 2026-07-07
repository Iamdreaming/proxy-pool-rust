# Design: readonly-dev-smoke-runner-v1

## Recommended Approach

Implement a small Python runner that lives with the integration tooling and
reuses the existing `tests/integration` configuration and MCP client.

Proposed entry point:

```powershell
python tests/integration/readonly_dev_smoke.py --branch main --wait-ci
```

Useful flags:

- `--branch <name>`: branch to inspect in GitHub Actions, default `main`.
- `--workflow <file>`: workflow file name, default `docker-build.yml`.
- `--skip-ci`: skip GitHub Actions inspection.
- `--wait-ci`: wait for the latest selected run to finish.
- `--json`: emit machine-readable summary in addition to the human output, or
  instead of it if the implementation keeps output simple.

The exact filename can change during implementation if an existing test helper
pattern suggests a better name, but the command should stay easy to run from the
repo root.

## Alternatives Considered

### Pytest wrapper

Run existing health and MCP tests directly:

```powershell
python -m pytest tests/integration/test_l1_health.py tests/integration/test_l4_mcp.py -q
```

This is cheap and already works, but it does not summarize CI state, does not
provide a single operational report, and includes broader MCP tool checks than
the post-push release validation needs.

### PowerShell script

A PowerShell script would fit the current local shell, but it would duplicate
HTTP/MCP parsing code and make later Linux/CI use less natural.

## Architecture

The runner should be organized around small check functions:

- `check_github_actions(...)`
- `check_http_status(...)`
- `check_http_readyz(...)`
- `check_mcp_service_status(...)`
- `check_mcp_update_status(...)`

Each check returns a simple result object:

- `name`
- `ok`
- `summary`
- `details`
- `triage_hint`

The main function aggregates results, prints a concise report, and exits `0`
only when all enabled checks are ok.

## Data Flow

1. Read CLI flags and existing `tests/integration/config.py` values.
2. Optionally run `gh run list` / `gh run watch` for the selected workflow and
   branch.
3. Query `http://{PROXY_POOL_HOST}:{PROXY_POOL_API_PORT}/api/status`.
4. Query `http://{PROXY_POOL_HOST}:{PROXY_POOL_API_PORT}/api/readyz`.
5. Use `McpClient` against `http://{PROXY_POOL_HOST}:{PROXY_POOL_MCP_PORT}/mcp`.
6. Call MCP `service_status`.
7. Call MCP `update_status`.
8. Compare runtime hashes with `PROXY_POOL_GIT_HASH` when provided.
9. Print summary and return an exit code.

## Contracts

The implementation should treat missing optional release fields as a failed
release-status check only when the current smoke explicitly depends on them.
The existing status contract expects:

- `/api/status.git_hash`
- `/api/status.release.git_hash`
- `/api/status.release.configured_image`
- `/api/status.release.update_enabled`
- MCP `service_status.git_hash`
- MCP `service_status.release.git_hash`
- MCP `update_status.status`

The runner must not rely on private Docker or SSH state.

## Error Handling

Each check should catch its own expected failures and return a failed result
instead of crashing the whole process:

- missing `gh`
- no matching workflow run
- GitHub Actions run failed or still in progress without `--wait-ci`
- HTTP timeout or connection failure
- non-JSON response
- HTTP status not in the expected set
- MCP initialization or tool-call failure
- git hash mismatch

Unexpected programmer errors may still raise during early development, but the
final runner should make normal operational failures readable.

## Testing Strategy

The implementation should be testable without live dev:

- Keep result formatting and decision logic in pure functions where practical.
- Add local tests around hash comparison, result aggregation, and exit code
  decisions, or at minimum run `python -m py_compile` for the runner and helper
  imports.
- Do not require real GitHub Actions, live HTTP, or live MCP for the local unit
  validation path.

Live validation, when the operator chooses to run it, uses the same public
surfaces already documented in `docs/dev-validation.md`.

## Rollout

This is additive. Existing `pytest` commands and manual `gh` checks continue to
work. Documentation should introduce the runner as the preferred shortcut while
keeping the underlying manual commands available for triage.

## Rollback

Remove the runner script and documentation references. No runtime service,
schema, deployment, or environment variable migration is involved.
