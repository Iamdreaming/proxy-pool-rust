# Implementation Plan: config-runbook-drift-check-v1

## Checklist

1. Load project specs with `trellis-before-dev`.
2. Update `docs/dev-validation.md`:
   - replace `release.update_image` with the actual release image fields;
   - add `PROXY_POOL_UPDATE_DOCKER_SOCKET` to the dev env expectations;
   - record the Watchtower `printenv` limitation and recommended alternatives.
3. Review README's dev validation paragraph and apply only a small sync if it
   needs to point more clearly at the runbook.
4. Add `tests/integration/test_l0_config_runbook_drift.py` with read-only text
   assertions for:
   - compose app update envs;
   - Watchtower token wiring;
   - documented release fields;
   - forbidden operator-doc drift such as `release.update_image`;
   - no-SSH / no-routine-update boundaries;
   - Watchtower shell-tool limitation.
5. Run focused validation:
   - `python -m py_compile tests\integration\test_l0_config_runbook_drift.py`
   - `python -m pytest tests\integration\test_l0_config_runbook_drift.py tests\integration\test_l0_no_ssh_helpers.py tests\integration\test_l0_readonly_dev_smoke.py -q`
6. Run `trellis-check` and fix any findings.
7. Decide whether this task creates reusable spec knowledge; update specs only
   if there is a non-obvious convention worth preserving.
8. Update `docs/ROADMAP.md` and task artifacts during closeout.
9. Commit task changes, excluding unrelated `.codex/config.toml`.

## Validation Results

- `python -m py_compile tests\integration\test_l0_config_runbook_drift.py`
  passed.
- `python -m pytest tests\integration\test_l0_config_runbook_drift.py tests\integration\test_l0_no_ssh_helpers.py tests\integration\test_l0_readonly_dev_smoke.py -q`
  passed with 17 tests.
- `git diff --check` passed for task changes; warnings only note local
  line-ending normalization.
- `trellis-check` found no required code changes beyond adding the integration
  testing spec for this drift-check pattern.

## Rollback Points

- If the drift test becomes too broad, reduce it to the env/status/runbook
  fields in the PRD rather than expanding into a full API contract smoke.
- If README encoding or unrelated text is unstable, avoid editing README and
  keep the authoritative runbook in `docs/dev-validation.md`.
- If validation shows runtime Rust drift, return to planning before widening
  scope beyond docs/tests.
