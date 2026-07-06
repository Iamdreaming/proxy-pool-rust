# Design: xray node lifecycle MVP

## Shape

Add shared xray lifecycle models to `proxy-core` so `proxy-xray` can write the
state while `proxy-api`, `proxy-mcp`, and `proxy-server` can read it without
introducing reverse dependencies.

Proposed core model:

- `XrayNodeLifecycleState`: `Pending`, `Activating`, `Active`, `Failed`,
  `Removed`.
- `XrayNodeStatus`: tag, protocol label, remote host/port, optional local
  SOCKS5 port, lifecycle state, optional last error, and updated timestamp.
- `XrayStatusSnapshot`: active/failed/removed/total counts plus recent nodes.
- `XrayStatusRegistry`: an `Arc<RwLock<...>>` friendly registry with helpers for
  state transitions and snapshot rendering.

## Data Flow

`proxy-server` creates one shared registry when xray is enabled and passes it to:

- `OutboundSync`, which writes lifecycle transitions.
- `proxy-api::AppState`, which exposes `/api/xray/status` and service status.
- `ProxyPoolMcpConfig`, which exposes the same snapshot through MCP.

When xray is disabled, API/MCP return an empty snapshot with `enabled: false` or
equivalent zero counts. Existing `active_nodes` remains for backward
compatibility during this slice.

## OutboundSync Transitions

`OutboundSync::sync_once()` becomes stricter about activation:

1. Build the node tag from protocol, host, and port.
2. Mark the node `activating` before allocating/configuring.
3. If port allocation or config generation fails, mark `failed` and continue.
4. Apply inbound and outbound config through xray gRPC. If either required step
   fails, release the port, mark `failed`, and do not store the proxy as active.
5. Store the local SOCKS5 proxy in `ProxyStore`. If storage fails, release the
   port, best-effort remove xray config, mark `failed`, and continue.
6. Insert the active node and mark lifecycle `active`.
7. When a stale node is removed, mark lifecycle `removed` with timestamp.

## API/MCP

`/api/xray/status` keeps `active_nodes` for compatibility and adds lifecycle
counts and `recent_nodes`. MCP should expose matching JSON. `service_status`
adds `failed_nodes` while preserving `active_nodes`.

## Testing

Unit tests cover the registry transitions and snapshot counts in `proxy-core`.
`proxy-xray` tests cover helper behavior around activation failure classification
without needing a real xray process. Existing API/MCP serialization tests are
updated to prove the new fields are present.
