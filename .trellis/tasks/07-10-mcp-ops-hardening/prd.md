# MCP ops hardening: async update_service + container logs tool

## Goal

Two operability fixes on the standalone MCP service, discovered during the split
rollout:
1. `update_service` blocks the MCP call for the full `docker pull` (can be
   minutes), so the client waits too long. Make it non-blocking.
2. There is no way to view container logs without SSH. Add an MCP tool that
   fetches recent logs via the Docker socket the MCP container already mounts.

## Requirements

### R1 — non-blocking update_service
- `update_service` validates config (enabled + token) synchronously, then sets
  status = `in_progress`, spawns the inspect→pull→Watchtower work as a detached
  background task, and returns immediately with `{"status":"update_started"}`.
- `update_status` reports the live state: `in_progress` while running, then the
  terminal `updated` / `already_current` / `failed` / `disabled`.
- Add `UpdateStatusKind::InProgress`.
- The background task records the terminal snapshot into the shared
  `Arc<RwLock<UpdateStatusSnapshot>>` (works because MCP is now its own process
  and is not killed by the Watchtower restart of proxy-pool).
- A safety timeout bounds the whole background update (default 300s) so a hung
  pull cannot leave status stuck in `in_progress` forever; on timeout record
  `failed`.

### R2 — container logs MCP tool
- New tool `container_logs` with params `{ container?: string, tail?: number }`.
  - `container` default = the configured update target (`PROXY_POOL_UPDATE_CONTAINER`,
    i.e. `proxy-pool`); allows viewing the main service's logs from the MCP box.
  - `tail` default 200, clamped to [1, 1000].
- Fetches `GET /containers/{id}/logs?stdout=1&stderr=1&tail=N&timestamps=1` over
  the Docker Unix socket and returns demuxed text (stdout+stderr merged in order).
- Handles Docker's 8-byte multiplexed stream framing (non-TTY containers).
- Structured error (never hang/panic) on socket failure / unknown container /
  non-Unix platform.

## Constraints

- proxy-mcp only; no proxy-api / proxy-server / deploy changes required (the MCP
  container already mounts `/var/run/docker.sock`).
- Reuse existing docker-socket helpers; add a raw (non-JSON) reader for the log
  stream (close/EOF-delimited).
- No new heavy deps.

## Out of scope

- Log streaming/follow (one-shot tail only).
- Auth/RBAC on logs (same trusted network as update_service).
- Fixing the pre-existing `read_http_response` chunked terminal-scan heuristic
  beyond what the raw log reader needs.

## Acceptance Criteria

1. `cargo test` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
2. `update_service` returns `{"status":"update_started"}` promptly (no pull wait);
   `update_status` transitions `in_progress` → terminal. Unit test for the state
   transitions / snapshot shapes.
3. `container_logs` returns demuxed text for a tail request; unit test for the
   frame-demux helper (pure function over a synthetic multiplexed buffer) and
   tail clamping.
4. Post-deploy (observation): calling `update_service` from the MCP client returns
   immediately; polling `update_status` shows progress then `updated`; proxy-pool
   `git_hash` changes. `container_logs` returns recent proxy-pool log lines.
