# Design: xray config dry-run and remove

## Shape

Add a small operator layer in `proxy-xray` that reuses `ConfigGenerator`,
`OutboundSync`, `XrayClient`, `PortManager`, `ProxyStore`, and
`XrayStatusRegistry`. API and MCP should call this layer instead of
re-implementing xray tag/config logic.

Proposed public models in `proxy-xray`:

- `XrayConfigDryRunRequest`: candidate `SubscriptionProxy` plus optional local
  SOCKS5 port.
- `XrayConfigDryRunResult`: status, tag metadata, fragment-presence booleans,
  and optional error.
- `XrayRemoveNodeResult`: status, tag, protocol/remote/local metadata if found,
  booleans for removed inbound/outbound/proxy entry, and optional error.
- `XrayOpsHandle`: cloneable shared handle for API/MCP operator actions.

## Data Flow

`proxy-server` constructs `XrayOpsHandle` only when xray is enabled, after the
same `XrayClient`, `PortManager`, `ProxyStore`, `OutboundSync`, and
`XrayStatusRegistry` exist. It passes the handle to `proxy-api::AppState` and
`ProxyPoolMcpConfig`.

Dry-run is pure: generate the same `XrayNodeConfig` that activation would use,
then return sanitized metadata. It never calls `XrayClient`, never allocates a
real port, and never writes stores.

Manual removal delegates to `OutboundSync` so the active in-memory map remains
the source of truth. `OutboundSync` removes the node from its active map,
best-effort removes xray inbound/outbound, releases the port, removes the local
SOCKS5 proxy from `ProxyStore`, and marks the lifecycle registry as removed.

## API/MCP

REST endpoints:

- `POST /api/xray/config/dry-run`
- `POST /api/xray/nodes/{tag}/remove`

MCP tools:

- `xray_config_dry_run`
- `xray_remove_node`

When xray is disabled or the handle is unavailable, REST returns `503` and MCP
returns structured JSON with `status: unavailable`.

## Error Handling

Dry-run returns `unsupported` for Basic/Unknown nodes or malformed JSON. Removal
returns `not_found` for unknown tags, `error` for disconnected xray or store
failures, and `removed` when the in-memory node is removed even if xray cleanup
only partially succeeds. Responses avoid full xray JSON and node secrets.

## Testing

Unit tests cover dry-run result sanitization, unsupported nodes, remove result
serialization, and tag helper behavior. Existing xray activation tests prove
background behavior stays compatible. API/MCP tests cover serialization,
parameter parsing, route/tool presence, and unavailable/not-found contracts.
