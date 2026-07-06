# Design: release-observability-no-ssh-v2

## Status metadata

Keep release metadata close to the existing status model in `proxy-core`. The
status response is already shared by REST and MCP, so adding a nested
`release` field there keeps contract ownership centralized.

Sources:

- Existing app version and git hash from compile/runtime configuration.
- Update-related runtime environment variables already used by update tooling.
- Optional build-time environment variables when available.

The release metadata must not require Docker socket access. It is safe to return
configured image/container names because the existing compose already exposes
similar values through env.

## Last update state

Keep update attempt memory inside the MCP update module because `update_service`
is MCP-only today. Use a small process-local shared state handle so:

- `update_service` writes the latest state before returning.
- `update_status` reads the latest state.
- Disabled and never-triggered states are explicit.

The stored payload should reuse the public update result shape where possible
instead of inventing a second reporting vocabulary.

## Public API/MCP contract

REST:

- `/api/status.release` contains release metadata.

MCP:

- `service_status.release` is automatically present through the shared status
  response.
- `update_status` returns a read-only latest update status object.

## Testing

- Unit-test release metadata construction from env/build values.
- Unit-test update status memory transitions without touching Docker.
- Existing API/MCP tests should continue compiling against the expanded status
  contract.

No integration step should rely on SSH.
