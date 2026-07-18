# Design: Business Availability E2E Smoke

## Boundary

This task adds a first-layer business availability smoke. It is intentionally
observational: it measures gateway and proxy-candidate reachability without
refreshing, cleaning, deleting, updating, or reconfiguring the service.

The implementation touches three areas:

- `proxy-core::validator`: make check-matrix targets support either legacy URL
  strings or structured targets with explicit expected HTTP statuses.
- `proxy-mcp`: mirror the same `check_proxy_matrix` input shape for MCP.
- `tests/integration`: add a local runner and focused unit tests for business
  target classification and report shape.

Roadmap/docs are updated only to reflect the new business availability priority.

## Data Flow

1. Gateway smoke:
   - Build an HTTP-proxy client pointing at `PROXY_POOL_HOST:PROXY_POOL_GW_PORT`.
   - Request each business target URL through the gateway.
   - Classify success by target-specific status rules.
   - Record status code, elapsed time, error message, and expected status rule.

2. Proxy candidate smoke:
   - Query `/api/proxies/scores` for the top candidates per protocol.
   - Convert each candidate into a `/api/proxy/check-matrix` request.
   - Send the default structured business targets.
   - Reuse the server's `ProxyCheckResult` diagnostics for per-target success,
     HTTP status, timings, exit IP/country, and error type.

3. Report:
   - Emit a concise human report by default.
   - Emit deterministic JSON with `--json` for later CI or dashboard reuse.
   - Exit non-zero only when enabled checks produce no useful business
     reachability signal. This keeps a noisy public target from hiding the
     entire picture while still failing when the gateway/proxy pool is totally
     unusable.

## Compatibility

`ProxyCheckMatrixRequest.targets` keeps the existing JSON form:

```json
["https://www.cloudflare.com/cdn-cgi/trace"]
```

It also accepts the new structured form:

```json
[
  {
    "url": "https://api.openai.com/v1/models",
    "expected_statuses": [401]
  }
]
```

Internally both forms normalize into `ValidationTarget`.

## Error Handling

- Invalid matrix target URLs still return the existing deterministic HTTP 400 /
  MCP `{ "status": "error", "message": ... }` shape.
- Runner network failures are captured as per-check failures, not uncaught
  tracebacks.
- Public target-specific blocks such as Reddit `403` or `429` are only
  considered successful when explicitly configured for that target.

## Operational Notes

- No direct SSH, host Docker, or mutating MCP/API tools are used.
- The first runner is a diagnostic smoke, not an auto-remediation loop.
- Future tasks can use the JSON output to drive target-aware routing, source
  quality ranking, or dashboard summaries.
