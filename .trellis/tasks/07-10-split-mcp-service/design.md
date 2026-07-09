# Design: standalone REST-client MCP service

## Architecture

Two containers on the same Docker network:

```
                 ┌──────────────────────────┐
  MCP client ───▶│ proxy-mcp (NEW)          │
  (Claude)  :9000│  - HTTP MCP transport    │
                 │  - RestClient ──REST────▶ proxy-pool :8000 /api/*
                 │  - local: geoip(MMDB+Redis),
                 │    check_proxy(matrix)   │
                 │  - update_service ─docker.sock/Watchtower─▶ recreate proxy-pool
                 └──────────────────────────┘
                 ┌──────────────────────────┐
                 │ proxy-pool (main)        │
                 │  API:8000 Gateway:9080   │
                 │  scheduler/sub/xray       │  (no longer serves MCP)
                 └──────────────────────────┘
```

Because the MCP process is a *sibling* of `proxy-pool`, triggering
`update_service` recreates `proxy-pool` while MCP keeps running and returns a
terminal result — fixing the hang.

## Tool routing (from research; 22 tools)

- **Via REST** (all in-proc-state + store-scored tools): get_proxy,
  get_best_proxy, list_proxies, explain_proxy_scores, pool_status, proxy_stats,
  cleanup_low_score_proxies, remove_proxy, service_status, xray_status,
  warp_status, refresh_pool, fetcher_status, subscription_sources,
  refresh_subscription_source, refresh_fetcher, route_test.
  Rationale for routing store reads via REST (not direct Redis): avoids
  duplicating `ScoreWeights`/`min_score` in the MCP process (scoring/ordering
  would otherwise drift).
- **Local in MCP process**: check_proxy, check_proxy_matrix (pure outbound
  network), geoip_lookup (local MMDB + Redis cache).
- **Local Docker/Watchtower**: update_service, update_status.

## Backend abstraction (single set of #[tool] defs)

Keep one `ProxyPoolMcp` with the existing `#[tool]` methods and `ToolRouter<Self>`.
Replace the in-proc handle fields with a REST client; keep local capabilities as
direct fields:

```rust
#[derive(Clone)]
pub struct ProxyPoolMcp {
    rest: RestClient,                    // NEW: reqwest wrapper, base = MCP_UPSTREAM_API_URL
    geoip: Option<Arc<Mutex<GeoIPLookup>>>,   // local (MMDB + Redis)
    update_status: Arc<RwLock<UpdateStatusSnapshot>>, // local
    git_hash: &'static str,
    started_at: Instant,
    tool_router: ToolRouter<Self>,
}
```

Removed fields: `store`, `balancer`, `scheduler_handle`, `subscription_ops`,
`route_selector`, `xray_status` — their tool bodies now call `self.rest`.

`ProxyPoolMcpConfig` becomes `{ upstream_api_url, geoip, git_hash, started_at }`.

### RestClient (`crates/proxy-mcp/src/rest_client.rs`)

```rust
pub struct RestClient { base: String, http: reqwest::Client }
impl RestClient {
    pub fn new(base: impl Into<String>) -> Self { /* 15s timeout */ }
    async fn get_json(&self, path: &str, query: &[(&str,String)]) -> Result<Value, RestError>;
    async fn post_json(&self, path: &str, body: &Value) -> Result<Value, RestError>;
    async fn delete_json(&self, path: &str) -> Result<Value, RestError>;
}
```
- 15s request timeout; on transport error / non-2xx, return a structured
  `RestError` that each tool maps to `{"status":"error","message":...}` JSON
  (never panic, never hang).
- Pure helpers `build_url(base, path, query)` and `map_status(code)` are unit
  tested without network.

Tool bodies become thin: e.g. `refresh_pool` → `POST /api/proxies/refresh`;
`route_test` → `GET /api/routes/test?host=&protocol=`; `service_status` →
`GET /api/status`; `xray_status` → `GET /api/xray/status`; `warp_status` →
`GET /api/warp`. Response JSON is reshaped to each tool's current output only
where today's shape differs from the REST body (documented per tool in
implement.md).

## New REST endpoints (`proxy-api`, R6)

1. `POST /api/proxies/cleanup` body `{protocol?,limit?,min_score?,apply?}` →
   `CleanupLowScoreResult` (calls `store.cleanup_low_score`). Mirrors
   `cleanup_low_score_proxies` args.
2. `remove_proxy` parity: MCP's remove does `store.mark_failed` (not plain
   delete). Add `POST /api/proxy/{key}/mark-failed` → returns updated/removed
   status. (Leave existing `DELETE /api/proxy/{key}` untouched.)
3. `proxy_stats` Socks4 count: `/api/status.pool` omits socks4. Either add
   `socks4` to the pool count block in `status.rs`, or have MCP compute
   `proxy_stats` from `GET /api/proxies?protocol=socks4` count. Choose extending
   `status.rs` pool block (single source of truth).

Key parsing for `{key}` must handle IPv6 (existing delete_proxy has an IPv6 bug,
out of scope here — new endpoint should use the same key parser to stay
consistent; note the limitation).

## Standalone binary (`crates/proxy-mcp/src/bin/proxy-mcp-server.rs`)

- Reads env: `MCP_UPSTREAM_API_URL` (default `http://proxy-pool:8000`),
  `MCP_HTTP_PORT` (default 9000), geoip settings (MMDB path + Redis url),
  `PROXY_POOL_UPDATE_*` (unchanged).
- Builds `GeoIPLookup` if MMDB present + Redis reachable (else geoip_lookup
  returns a disabled result, matching today's `None` behavior).
- Serves the same axum `/mcp` streamable-http app currently in `main.rs:369-426`
  (move that block into a reusable `serve_http(mcp, port)` in proxy-mcp).

`proxy-mcp` gains a `reqwest` dependency and a `[[bin]]` target; it keeps
`proxy-core` (geoip, models, validator) but drops direct use of the in-proc
handle types in tool bodies.

## Main service change (`proxy-server/main.rs`, R5)

Remove: `ProxyPoolMcp` construction (205-215), the MCP HTTP block (369-427),
the stdio block (428-441), and MCP from the `tokio::select!`. Keep everything
else. `proxy-server` no longer depends on `proxy-mcp`.

## Deployment (`deploy/`, R8)

- New `deploy/Dockerfile.mcp` (or a build arg/target) producing the
  `proxy-mcp-server` binary image.
- `docker-compose.yml`: add `proxy-mcp` service — image, `container_name:
  proxy-mcp`, `ports: ["9000:9000"]`, mounts `/var/run/docker.sock`, `../config`
  (for MMDB), env `MCP_UPSTREAM_API_URL=http://proxy-pool:8000` +
  `PROXY_POOL_UPDATE_*` (moved here) + Redis url for geoip, `depends_on:
  [proxy-pool, redis]`, `watchtower.enable=true` label so it too can be updated.
  Remove `"9000:9000"` from `proxy-pool` and its `PROXY_POOL_UPDATE_*` env
  (moves to proxy-mcp). Keep proxy-pool's docker.sock (WARP optimizer still uses
  it) — verify.
- `PROXY_POOL_UPDATE_CONTAINER` stays `proxy-pool` (MCP updates its sibling).

## Compatibility / rollout / rollback

- `.mcp.json` clients keep pointing at `:9000` — unchanged URL, now served by the
  new container.
- Rollout: build both images; `docker compose up -d` brings up proxy-mcp; MCP URL
  identical. No API/gateway change.
- Rollback: revert compose to embed MCP in proxy-pool (re-add 9000 mapping, drop
  proxy-mcp service). Code is additive to proxy-mcp; main.rs change is the only
  removal — keep it in a revertable commit.

## Risks

- REST latency/availability: MCP tools now depend on proxy-pool being up. Mitigate
  with timeouts + structured errors (a down main service yields tool errors, not
  hangs). Acceptable — MCP is an ops/debug surface.
- geoip config drift: MCP builds its own `GeoIPLookup`; keep geoip settings in the
  shared `config/settings.yaml` (same mount) so both processes agree.
- `route_test` accuracy: it reflects the gateway's live failover metrics, which
  live in proxy-pool — REST is the only correct source (confirmed). No local path.
