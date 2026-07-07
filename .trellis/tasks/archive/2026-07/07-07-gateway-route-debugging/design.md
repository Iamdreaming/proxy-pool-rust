# Design: Gateway Route Debugging

## Scope

This task adds a structured explanation layer around gateway route selection and exposes it through API, MCP, logs, metrics, and tests. The gateway remains a HTTP CONNECT + SOCKS5 proxy; clients should not see protocol changes except for optional diagnostics when explicitly enabled.

## Architecture

### Core Decision Types

Create route decision types in the shared routing/selection layer, most likely near `proxy-gateway::upstream` unless a `proxy-core` type is needed by both API and MCP without depending on gateway internals.

Proposed public shapes:

- `RouteTestParam`: host, optional protocol.
- `RouteDecision`: host, protocol, matched_group, matched_reason, geoip summary, candidates, selected, unavailable.
- `RouteCandidate`: exit type, priority, source, available boolean, reason.
- `RouteExit`: direct, free_pool, warp, xray, no_proxy.
- `GatewayAttempt`: exit, status, error, elapsed_ms.

`Upstream` should stay as the runtime connection target. A helper can convert a selected `RouteCandidate` into an `Upstream` when a concrete pool proxy, WARP port, or xray port is available.

### Selector Flow

Add a traceable selector method beside the existing `select`:

- `select_with_trace(host, protocol) -> RouteSelection`
- `dry_run(host, protocol) -> RouteDecision`

`RouteSelection` contains the existing final `Upstream` plus its `RouteDecision`.

The existing `select` can delegate to `select_with_trace` and return only `upstream`, preserving current callers while allowing HTTP/SOCKS handlers to opt into traces.

Candidate order should reflect current behavior:

- explicit `direct`: direct only
- explicit `free_pool`: pool, then WARP, then xray, then no_proxy
- explicit `warp`: WARP, then xray, then pool, then no_proxy
- explicit `xray`: xray, then pool, then WARP, then no_proxy
- GeoIP domestic: direct
- GeoIP overseas: WARP, then xray, then pool, then no_proxy
- no router/no GeoIP/general fallback: pool, then WARP, then xray, then no_proxy

The design intentionally records candidate availability separately from network connection success. Availability means the selector found a pool proxy, WARP instance, or active xray node.

### Runtime Fallback

HTTP CONNECT and SOCKS5 handlers should use `select_with_trace`. The minimal acceptable implementation records the final selected upstream and connection success/failure. The preferred implementation attempts later candidates when a selected upstream fails to connect, because the roadmap explicitly calls for fallback chain debugging.

For retry-capable behavior:

1. Build candidate list.
2. For each concrete available candidate, try to establish the tunnel.
3. Record `GatewayAttempt` with success/failure and error message.
4. Stop on first success.
5. Return 502 / SOCKS failure only after all available candidates fail or no candidate exists.

Direct candidates should only be retried according to the candidate list; there is no loop back to direct after proxy fallback.

### API Surface

Add a route dry-run endpoint, for example:

- `GET /api/routes/test?host=example.com&protocol=http`

Response:

```json
{
  "status": "ok",
  "decision": {
    "host": "example.com",
    "protocol": "http",
    "matched_group": "free_pool",
    "candidates": [],
    "selected": "free_pool"
  }
}
```

The endpoint should not open a tunnel to the target. Bad input returns HTTP 400 with structured JSON.

### MCP Surface

Add `route_test` with parameters:

- `host`: required
- `protocol`: optional, defaults to `http`

Return the same decision structure as the API endpoint.

### Metrics

Add gateway metrics with stable names. If the current status metrics remain snapshot-based, add a small process-local counter registry in `proxy-gateway` or shared app state and render it from `/api/metrics`.

Minimum labels:

- `exit`: `direct`, `free_pool`, `warp`, `xray`, `no_proxy`
- `status`: `success`, `failure`, `unavailable`
- `protocol`: `http_connect`, `socks5`

### Logging

Emit structured tracing fields for gateway attempts:

- target host
- protocol
- matched group
- exit type
- attempt status
- error message on failure

Logs are supporting evidence only; API/MCP and metrics remain the primary operator surfaces.

## Compatibility

- Existing proxy client behavior remains compatible.
- Existing `UpstreamSelector::select` remains available.
- Route dry-run is additive.
- Metrics are additive.
- No Redis or configuration migration is required.

## Risks

- Retrying after a partially established tunnel is unsafe; retry only before returning success to the client.
- Pool proxy candidates are currently chosen randomly through `ProxyStore`. Dry-run should not promise exact future random selections unless it returns only candidate type, or it should clearly label sampled proxy identity as advisory.
- API/MCP layers need access to the selector or a route-debug service. If `AppState` does not currently hold it, add the smallest shared handle needed instead of duplicating selection logic.

## Rollback

Revert the task commit. New route-test endpoints, MCP tool, metrics, and trace types are additive and do not change stored data.
