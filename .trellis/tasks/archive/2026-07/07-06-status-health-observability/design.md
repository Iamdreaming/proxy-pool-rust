# Design: status-health-observability

## Scope

This task adds a status and health surface for operators. It touches:

- `proxy-api`: HTTP routes, response contracts, status assembly.
- `proxy-mcp`: `service_status` tool.
- `proxy-server`: AppState wiring for uptime/start time if needed.
- `deploy/docker-compose.yml`: container healthcheck.
- `tests/integration`: deployed-instance checks.

Dashboard UI is intentionally excluded.

## Status Model

Use one shared status shape for API and MCP as much as practical:

```json
{
  "version": "0.1.0",
  "git_hash": "a2436f1",
  "uptime_sec": 1234,
  "pool": {
    "http": 10,
    "https": 0,
    "socks5": 5,
    "total": 15
  },
  "redis": {
    "status": "ok"
  },
  "warp": {
    "configured": 3,
    "healthy": 2
  },
  "xray": {
    "active_nodes": 12
  }
}
```

For degraded Redis state, return an explicit error summary instead of hiding the
failure behind zero counts:

```json
{
  "redis": {
    "status": "error",
    "message": "redis operation failed"
  }
}
```

## Endpoint Semantics

### `/api/healthz`

- Process liveness only.
- Does not touch Redis.
- Intended for Docker healthcheck.

### `/api/readyz`

- Required dependency readiness.
- Checks Redis with a cheap operation.
- Returns 503 when Redis is not usable.

### `/api/status`

- Human/operator status summary.
- Can return 200 with degraded sections because the process is still reachable.
- Must surface dependency failures explicitly.

### `/api/metrics`

- Prometheus text format.
- Avoid external network calls.
- Convert dependency failures into status/error metrics, not fake zeros.

## MCP Design

`service_status` should call shared status assembly logic or use the same helper
contract used by API routes. `proxy-mcp` must remain an adapter layer; it should
not reimplement pool counting or dependency-status semantics independently.

## Docker Healthcheck

Use `/api/healthz` for container health by default. Redis outages should be
reported through readiness and metrics, not cause a tight restart loop.

## Compatibility

- Existing `/api/status.pool.http|https|socks5` fields should remain available.
- Additive JSON fields are acceptable.
- Existing metrics names should remain available.
- Existing MCP tools remain unchanged; `service_status` is additive.

## Risks

- If status assembly lives only in `proxy-api`, MCP may duplicate logic. Prefer a
  small shared helper/module where dependency boundaries allow it.
- Readiness checks must be cheap; avoid triggering fetch/validate cycles.
- Docker healthcheck must not be stricter than process liveness unless we want
  automatic restarts on dependency outages.

