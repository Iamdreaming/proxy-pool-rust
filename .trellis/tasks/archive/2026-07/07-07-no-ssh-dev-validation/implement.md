# Implementation Plan: No SSH Dev Validation

## Phase 1: Planning

- [x] Create Trellis task.
- [x] Inspect Roadmap, current task state, integration helpers, integration tests, GitHub Actions, and project workflow notes.
- [x] Write PRD, design, and implementation plan.

## Phase 2: Documentation

- [x] Add `docs/dev-validation.md` with the allowed no-SSH validation checklist.
- [x] Update `CLAUDE.md` to state the no-SSH rule and link to the checklist.
- [x] Ensure docs distinguish container-internal Docker socket use from external host control.

## Phase 3: Helper Hardening

- [x] Update `tests/integration/helpers/docker_control.py` docstrings to remove SSH/direct Docker assumptions.
- [x] Add a dedicated exception for unavailable fault injection.
- [x] Make WARP stop/start helpers raise that exception instead of silently passing.
- [x] Keep API-only cleanup helper behavior.

## Phase 4: Verification

- [x] `python -m py_compile tests/integration/helpers/docker_control.py tests/integration/config.py tests/integration/conftest.py tests/integration/test_l1_health.py tests/integration/test_l4_mcp.py`
- [x] Run a lightweight Python check that WARP fault-injection helpers raise the expected exception.
- [x] `git diff --check`
- [x] Confirm no SSH command is used during this task.

## Phase 5: Spec And Roadmap

- [x] Add deployment validation reminder to `.trellis/spec/guides/index.md`.
- [x] Move `no-ssh-dev-validation` to Roadmap Done.
- [x] Promote `score-retention-policy` to Roadmap Now.

## Risk Points

- Fault-injection tests must not be presented as covered until a safe MCP/API control surface exists.
- Documentation must not imply `docker.sock` inside the container is available to external test runners.
