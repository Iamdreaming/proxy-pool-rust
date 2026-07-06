# WARP ops enhancement

Status: paused. The user asked to stop pursuing this direction for now on 2026-07-07. Keep this task as a later planning stub only; do not start implementation unless the user explicitly resumes WARP work.

## Goal

Expose richer WARP operational status across API, MCP, and dashboard without fake controls.

## Requirements

- Improve WARP instance status with real operational fields such as endpoint, latency, loss, health, assignment time, and failure count.
- Expose recent optimizer result and failure reason through API/MCP.
- Keep Web WARP actions truthful: show only actions backed by API/MCP support.

## Acceptance Criteria

- [ ] API/MCP can query current WARP instances, recent optimizer result, and failure reason.
- [ ] Web WARP page does not show unsupported fake controls.
- [ ] `cargo test --workspace --all-targets` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
