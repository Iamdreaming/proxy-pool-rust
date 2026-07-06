# Implementation Plan: status-health-observability

## Order of Work

1. Inspect current API/MCP status code paths.
   - `crates/proxy-api/src/routes.rs`
   - `crates/proxy-api/src/lib.rs`
   - `crates/proxy-mcp/src/lib.rs`
   - `crates/proxy-server/src/main.rs`

2. Add shared status structures/helpers.
   - Preserve existing `/api/status.pool.http|https|socks5` compatibility.
   - Add total, Redis, WARP, xray, uptime fields.
   - Avoid swallowing Redis errors.

3. Add API routes.
   - `GET /api/healthz`
   - `GET /api/readyz`
   - expanded `GET /api/status`
   - expanded `GET /api/metrics`

4. Wire uptime/start time.
   - Add start timestamp to `AppState` or equivalent wiring.
   - Initialize in `proxy-server` at startup.

5. Add MCP `service_status`.
   - Reuse shared status assembly where practical.
   - Add tool-list and output tests.

6. Add Docker healthcheck.
   - Use `/api/healthz` for process liveness.
   - Keep `/api/readyz` for readiness gates.

7. Update tests.
   - Unit tests for response serialization/helper behavior.
   - Integration tests for `/api/healthz`, `/api/readyz`, expanded `/api/status`.
   - MCP expected tool list includes `service_status`.

8. Update docs if user-visible behavior changes.
   - README API endpoint table.
   - Roadmap status if completing the task.

## Validation Commands

Run before commit:

```powershell
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
npx vue-tsc --noEmit
npm run build
docker compose config
```

Optional deployed-instance validation:

```powershell
pytest tests/integration/test_l1_health.py -v
pytest tests/integration/test_l2_api.py -v
pytest tests/integration/test_l4_mcp.py -v
```

## Risk Checks

- Confirm Redis unavailable behavior manually or via test harness if possible.
- Confirm Docker healthcheck does not create restart loops during Redis outages.
- Confirm `/api/status` remains backward compatible for current web store usage.

