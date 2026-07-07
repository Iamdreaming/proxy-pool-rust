# Read-only Dev Smoke Runner

## Scenario: Post-push no-SSH smoke command

### 1. Scope / Trigger

Trigger this contract when adding or changing a local command that validates the
dev deployment after a push. The command may inspect CI and public service
state, but it must not mutate the deployment.

### 2. Signatures

Primary command:

```powershell
python tests\integration\readonly_dev_smoke.py --branch main --wait-ci
```

Useful variants:

```powershell
python tests\integration\readonly_dev_smoke.py --skip-ci
python tests\integration\readonly_dev_smoke.py --json
```

Supported environment variables come from `tests/integration/config.py`:

- `PROXY_POOL_HOST`
- `PROXY_POOL_API_PORT`
- `PROXY_POOL_GW_PORT`
- `PROXY_POOL_MCP_PORT`
- `PROXY_POOL_GIT_HASH`

### 3. Contracts

Allowed read-only surfaces:

- GitHub Actions latest `docker-build.yml` run through `gh run list` and
  optionally `gh run watch`.
- REST `GET /api/status`.
- REST `GET /api/readyz`.
- MCP `service_status`.
- MCP `update_status`.

Forbidden surfaces and actions:

- Direct SSH to the dev address.
- Host Docker CLI/API access.
- MCP `update_service`.
- Pool refresh, proxy deletion, cleanup apply, subscription apply, or any other
  mutating operation.

Command result contract:

- Exit `0` only when every enabled check passes.
- Exit non-zero when any enabled check fails.
- `--skip-ci` disables only the GitHub Actions check; HTTP and MCP checks still
  run.
- Missing `PROXY_POOL_GIT_HASH` means runtime hash comparison is skipped.
- Set `PROXY_POOL_GIT_HASH` to require both HTTP and MCP status hashes to start
  with the expected value.

### 4. Validation & Error Matrix

| Condition | Result |
|-----------|--------|
| `gh` unavailable and CI enabled | Fail with hint to install/authenticate `gh` or use `--skip-ci` |
| Latest workflow run not completed | Fail with `gh run watch <id> --exit-status` hint |
| Latest workflow conclusion is not success | Fail with `gh run view <id> --log-failed` hint |
| `/api/status` unavailable or non-200 | Fail with public API reachability hint |
| `/api/status` missing release fields | Fail with release contract smoke hint |
| Runtime hash mismatch | Fail with CI/update-status triage hint |
| `/api/readyz` returns 503 or non-`ok` status | Fail with readiness hint |
| MCP transport unavailable | Fail with MCP transport hint |
| MCP `update_status` tool missing | Fail with MCP contract hint |
| MCP `update_status.status=failed` | Fail with latest update failure hint |

### 5. Good/Base/Bad Cases

- Good: CI succeeded, HTTP status and MCP service status expose matching git
  hashes, readyz is `ok`, and update status is a known non-failed state.
- Base: CI is skipped with `--skip-ci`; HTTP and MCP checks still prove public
  state.
- Bad: The command calls `update_service` to repair stale dev state. The smoke
  runner must report the stale state instead and leave mutation to an explicit
  operator decision.

### 6. Tests Required

- Local pure tests for hash comparison and result aggregation.
- Local tests for status payload validation and missing field failures.
- Local tests or static assertions for the read-only MCP tool set.
- `python -m py_compile` for the runner and integration helper imports.
- Optional live read-only smoke against dev after local tests pass; live failure
  is acceptable when it accurately reports current public-surface state.

### 7. Wrong vs Correct

#### Wrong

```powershell
python tests\integration\readonly_dev_smoke.py
# script notices old git_hash and calls update_service automatically
```

This turns validation into deployment mutation and bypasses the operator
decision point.

#### Correct

```powershell
python tests\integration\readonly_dev_smoke.py --branch main --wait-ci
# script reports old git_hash and points to service_status/update_status triage
```

The operator can then decide whether an explicit update action is appropriate.
