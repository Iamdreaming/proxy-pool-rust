# Implementation Plan: readonly-dev-smoke-runner-v1

## Preconditions

- User approves the recommended Python runner approach.
- Do not restore any paused stash.
- Do not touch `.codex/config.toml`.
- Do not SSH to the dev address.
- Do not call MCP `update_service`.

## Steps

1. Inspect current integration helper patterns.
   - `tests/integration/config.py`
   - `tests/integration/helpers/mcp_client.py`
   - `tests/integration/test_l1_health.py`
   - `tests/integration/test_l4_mcp.py`

2. Add the runner script.
   - Prefer `tests/integration/readonly_dev_smoke.py`.
   - Use existing `PROXY_POOL_*` environment variables.
   - Add CLI flags for branch, workflow, CI wait/skip, and optional JSON output.
   - Keep all checks read-only.

3. Add local validation coverage.
   - Add a small unit-style test file if practical, for result aggregation and
     hash/status decision logic.
   - Otherwise, ensure the script can be imported and compiled without live dev.

4. Update documentation.
   - Add the command to `docs/dev-validation.md`.
   - Reference the shortcut from README's Dev validation section if it becomes
     the preferred post-push entry.
   - Keep manual `gh`, HTTP, and MCP commands documented for triage.

5. Update roadmap and task artifacts.
   - Mark `readonly-dev-smoke-runner-v1` Done in `docs/ROADMAP.md` after
     implementation and verification.
   - Move the next candidate to Now/Ready as appropriate.
   - Archive the Trellis task when complete.

## Validation Commands

Minimum local validation:

```powershell
python -m py_compile tests\integration\readonly_dev_smoke.py tests\integration\helpers\mcp_client.py tests\integration\config.py
```

If tests are added:

```powershell
python -m pytest tests\integration\<new-test-file>.py -q
```

Optional live read-only smoke, only through public surfaces:

```powershell
$env:PROXY_POOL_HOST = "100.64.0.2"
$env:PROXY_POOL_GIT_HASH = (git rev-parse --short HEAD)
python tests\integration\readonly_dev_smoke.py --branch main --wait-ci
```

Post-push:

```powershell
git push origin main
gh run list --workflow=docker-build.yml --branch main --limit 1
gh run watch <run-id> --exit-status
```

## Review Checklist

- [x] No direct SSH, host Docker CLI/API, or update-service call exists in the
      runner.
- [x] Failed checks return non-zero and include a triage hint.
- [x] Runtime hash comparison is optional when `PROXY_POOL_GIT_HASH` is unset.
- [x] CI check can be skipped for local/offline validation.
- [x] Documentation still preserves manual fallback commands.
- [x] `.codex/config.toml` remains unstaged.

## Validation Results

- `python -m py_compile tests\integration\readonly_dev_smoke.py tests\integration\test_l0_readonly_dev_smoke.py tests\integration\conftest.py tests\integration\helpers\mcp_client.py tests\integration\config.py` passed.
- `python -m pytest tests\integration\test_l0_readonly_dev_smoke.py -q` passed with 9 tests.
- `python tests\integration\readonly_dev_smoke.py --skip-ci --timeout 5` ran read-only and failed as expected against the current dev runtime because release fields and MCP `update_status` are absent.
- `python tests\integration\readonly_dev_smoke.py --timeout 5` verified the latest GitHub Actions run, then failed on the same live dev public-surface contract gaps.
- `python tests\integration\readonly_dev_smoke.py --skip-ci --timeout 5 --json` returned the same public-surface failures as structured JSON.

## Risks

- GitHub CLI output format can change. Prefer `gh run list --json ...` if
  available during implementation.
- Live dev can be temporarily unhealthy. Local validation should not require
  live dev unless the operator explicitly runs the live smoke command.
- MCP transport failures should produce a readable failed result, not a Python
  traceback during normal operation.
