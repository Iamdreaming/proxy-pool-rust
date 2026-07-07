<!-- TRELLIS:START -->
# Trellis Instructions

These instructions are for AI assistants working in this project.

This project is managed by Trellis. The working knowledge you need lives under `.trellis/`:

- `.trellis/workflow.md` — development phases, when to create tasks, skill routing
- `.trellis/spec/` — package- and layer-scoped coding guidelines (read before writing code in a given layer)
- `.trellis/workspace/` — per-developer journals and session traces
- `.trellis/tasks/` — active and archived tasks (PRDs, research, jsonl context)

If a Trellis command is available on your platform (e.g. `/trellis:finish-work`, `/trellis:continue`), prefer it over manual steps. Not every platform exposes every command.

If you're using Codex or another agent-capable tool, additional project-scoped helpers may live in:
- `.agents/skills/` — reusable Trellis skills
- `.codex/agents/` — optional custom subagents

Managed by Trellis. Edits outside this block are preserved; edits inside may be overwritten by a future `trellis update`.

<!-- TRELLIS:END -->

# Project-Specific Dev Update Policy

This project does not use direct SSH to the dev address as the default
deployment or validation path.

After pushing changes, use this order:

1. Wait for the GitHub Actions Docker image build to finish successfully.
2. Check public read-only surfaces first:
   - `GET /api/status`
   - `GET /api/readyz`
   - MCP `service_status`
   - MCP `update_status`
3. Compare the runtime `git_hash` / `release.git_hash` with the pushed short
   SHA.
4. Call MCP `update_service` only when an operator or user explicitly chooses
   to update dev and the read-only checks show dev is still on an old image.
5. If the `update_service` HTTP/MCP call disconnects while Watchtower replaces
   the container, treat that as inconclusive rather than failed; verify the
   final result through `/api/status.git_hash` and `/api/readyz`.

Do not use these as routine dev validation steps:

- Direct SSH to the dev address.
- Host Docker CLI/API access.
- Calling `update_service` as a status check.
- Refreshing, deleting, cleaning, or applying remote state unless the task
  explicitly requires that mutation.

The canonical runbook is `docs/dev-validation.md`; README and CLAUDE.md contain
shorter summaries of the same boundary.
