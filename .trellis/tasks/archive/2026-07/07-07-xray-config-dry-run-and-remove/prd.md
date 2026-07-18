# xray config dry-run and remove

## Goal

Expose safe xray config dry-run validation and manual single-node removal through API/MCP without direct dev SSH.

## Background

`xray-node-lifecycle-mvp` made activation state visible, but operators still do
not have a safe way to preflight a candidate encrypted node config or remove one
active xray node on demand. Today xray config changes happen inside
`OutboundSync` during background activation, and stale removal only happens when
the pending set changes.

## Requirements

### F1: Config Dry-Run

- Provide a dry-run operation for encrypted subscription nodes.
- Input accepts a `SubscriptionProxy` JSON payload plus an optional local
  SOCKS5 port.
- The operation validates that the node can produce inbound/outbound/routing
  JSON fragments without writing anything to xray-core, `ProxyStore`, or Redis.
- The response includes stable metadata: tag, inbound tag, outbound tag,
  protocol label, remote host/port, local port, and whether each fragment was
  generated.
- The response must not expose raw passwords, UUIDs, or full config JSON.

### F2: Manual Single-Node Removal

- Provide a removal operation for a currently tracked xray node by tag.
- Removal must best-effort remove inbound/outbound from xray-core, release the
  allocated local port, remove the active proxy entry from `ProxyStore`, update
  `XrayStatusRegistry`, and remove the node from the active in-memory set.
- Unknown tags return a structured not-found result and must not mutate state.
- If xray gRPC is disconnected, the operation returns a structured error
  instead of silently pretending success.

### F3: API and MCP Visibility

- REST exposes xray dry-run and removal endpoints.
- MCP exposes matching tools.
- API/MCP responses share the same underlying result models.

### F4: Compatibility and Boundaries

- Existing `OutboundSync` activation behavior remains compatible.
- No direct SSH/dev-host Docker validation.
- No subscription source CRUD or Web Dashboard integration in this slice.
- Removal is an operator action, not an automatic pending-store mutation.

## Acceptance Criteria

- [ ] API can dry-run a Shadowsocks/VMess/Trojan node and return sanitized
  generated-fragment metadata.
- [ ] MCP can perform the same dry-run operation.
- [ ] API can remove a known xray node by tag through a shared operator handle.
- [ ] MCP can perform the same remove operation.
- [ ] Unknown tags return a structured not-found response.
- [ ] Removal updates lifecycle status as `removed` and releases the local port.
- [ ] Existing xray activation tests still pass.
- [ ] `cargo test --workspace --all-targets` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.

## Verification

- `cargo fmt --all --check`
- `cargo test -p proxy-xray`
- `cargo check -p proxy-api -p proxy-mcp -p proxy-server`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- No-SSH post-push smoke through GitHub Actions and public HTTP/MCP surfaces.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
