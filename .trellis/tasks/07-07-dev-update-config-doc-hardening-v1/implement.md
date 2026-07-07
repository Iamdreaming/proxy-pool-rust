# Implementation Plan

## Checklist

- [x] Review compose, README, dev-validation runbook, and drift guard.
- [x] Expand `docs/dev-validation.md` with managed dev compose roles, labels,
      Watchtower HTTP update behavior, and explicit rollback/pause guidance.
- [x] Update README dev validation summary to point to self-update wiring docs.
- [x] Extend local drift checks for container roles, label intent, Watchtower
      command, no-SSH verification wording, and rollback/pause boundary.
- [x] Update integration testing spec if the drift-guard contract grows.
- [x] Run focused local pytest.
- [x] Update Roadmap and task status.

## Validation Commands

- `python -m pytest tests\integration\test_l0_config_runbook_drift.py -q`
- `python -m pytest tests\integration\test_l0_config_runbook_drift.py tests\integration\test_l0_release_status_public_smoke.py -q`

## Notes

- Keep the runbook operationally useful but do not turn rollback guidance into
  agent-executable SSH or host-Docker instructions.
- `.codex/config.toml` remains unrelated and must not be staged.
