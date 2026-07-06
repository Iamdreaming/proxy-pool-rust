# xray node lifecycle MVP

## Goal

Expose xray node lifecycle and activation failure reasons through core status, API, and MCP.

## Background

The current xray integration exposes only an `active_nodes` counter. `OutboundSync`
can warn when `add_inbound`, `add_outbound`, config generation, Redis storage, or
port allocation fails, but those failures are not visible through API/MCP and a
node can still look active after a partial xray configuration failure.

## Requirements

### F1: Lifecycle State

- Define an xray node lifecycle visible to operators: `pending`, `activating`,
  `active`, `failed`, and `removed`.
- Each visible node record includes tag, protocol label, remote host/port,
  optional local SOCKS5 port, state, last error, and updated timestamp.

### F2: Accurate OutboundSync Transitions

- Mark nodes as `activating` before xray config is applied.
- Mark nodes as `active` only after required xray config and pool storage steps
  succeed.
- Mark nodes as `failed` with a concise reason when config generation, port
  allocation, xray config, or pool storage fails.
- Mark stale nodes as `removed` when they are removed from the active set.

### F3: API and MCP Visibility

- `/api/xray/status` returns active/failed/removed counts and recent lifecycle
  records.
- MCP exposes the same lifecycle snapshot through an xray status tool or the
  existing service status surface.
- `/api/status` and `service_status` include at least active and failed counts.

### F4: Compatibility

- Existing xray happy path behavior remains compatible for gateway routing and
  pool status.
- `proxy-api` and `proxy-mcp` must not take a direct dependency on `proxy-xray`;
  shared status models belong in `proxy-core`.

## Non-goals

- Subscription source CRUD and manual refresh; that is `subscription-source-ops-mvp`.
- xray config dry-run and single-node removal actions; that is
  `xray-config-dry-run-and-remove`.
- WARP optimizer or WARP endpoint operations.
- Direct SSH/dev-host Docker validation.

## Acceptance Criteria

- [ ] `/api/xray/status` returns lifecycle counts and recent node records.
- [ ] MCP can return the same lifecycle snapshot.
- [ ] `active_nodes` is still available for existing clients.
- [ ] Failed activation paths preserve a reason visible through API/MCP.
- [ ] `cargo test --workspace --all-targets` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.

## Verification

- `cargo fmt --all --check`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- No-SSH smoke check after push, using GitHub Actions and public HTTP/MCP
  surfaces only.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
