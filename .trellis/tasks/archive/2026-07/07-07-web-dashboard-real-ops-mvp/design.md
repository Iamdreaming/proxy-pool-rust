# Design: web-dashboard-real-ops-mvp

## Approach

Implement the dashboard as an integration MVP over existing REST endpoints.
The first screen should be operationally useful, compact, and truthful: service
identity, dependency readiness, pool counts, WARP/xray summaries, and recent
proxy data. Secondary pages expose specific workflows without pretending to
support operations that do not exist yet.

## Boundaries

- `web/src/types/index.ts` owns frontend contracts for API payloads.
- `web/src/api/index.ts` owns HTTP calls and endpoint paths.
- Vue views own presentation, local form state, loading state, and user
  messages.
- Rust backend stays unchanged unless the frontend reveals a missing existing
  contract required for the MVP.

## Data Flow

```
proxy-core/proxy-api structs
  -> JSON REST endpoints
  -> web/src/api typed helpers
  -> Vue view state
  -> Naive UI tables/cards/forms
```

### Endpoints

- `GET /api/status` -> `StatusResponse`
- `GET /api/readyz` -> `DependencyStatus`
- `GET /api/proxies` -> `ProxiesResponse`
- `GET /api/proxies/scores` -> `ScoredProxiesResponse`
- `GET /api/routes/test?host=&protocol=` -> `RouteTestResponse`
- `GET /api/fetchers` -> `FetchersResponse`
- `POST /api/fetchers/{id}/refresh` -> `RefreshResponse`
- Existing simple proxy/status endpoints stay available for MCP Debug.

## UI Plan

### Dashboard

Replace demo-like summary cards with real status cards:

- Pool total and protocol counts
- Redis readiness and readyz result
- Version/git hash/uptime
- WARP configured/healthy and xray active nodes
- Recent HTTP proxies from existing `pool.loadProxies`

### Proxies

Keep current filters, table, and refresh action. Add score-aware mode by
loading `/api/proxies/scores` for the selected protocol and rendering score
fields in additional columns. The existing proxy list remains the base table if
score data is empty.

### Routes

Replace the current route rule editor with a real dry-run panel. The backend
does not currently expose route rule read/write endpoints, so the page must not
present save/add/delete controls as if they work. It should state that rules
are configured through `config/settings.yaml` for now, then expose host/protocol
dry-run against `/api/routes/test`.

### Fetchers

Add a top-level Fetchers page and sidebar entry. It lists all fetchers from the
real API and exposes a per-row refresh action. Refresh results update the table
and show returned counts.

### MCP Debug

Update tool catalog to current backend tools. For REST-backed tools, call REST
helpers. For MCP-only tools such as `update_service` and `cleanup_low_score`,
return a truthful transport notice instead of fake success.

### Logs

Remove the generated timer. Until a real log endpoint exists, render an empty
state that says live logs are unavailable in the web UI and direct operators to
real status/fetcher/route pages.

## Error Handling

- Each view has explicit loading state.
- API errors show `message.error(...)` and leave previous successful data in
  place when that is safer for operators.
- Empty API results render empty states, not fake rows.
- MCP-only actions return a warning result with `isError: true` in MCP Debug.

## Compatibility

No backend contract changes are planned. Existing routes and dashboard pages
remain available. The only navigation addition is `/fetchers`.

## Validation

- `npm run build`
- `cargo test -p proxy-api --lib` if backend API files change
- Focused code review against PRD acceptance criteria
