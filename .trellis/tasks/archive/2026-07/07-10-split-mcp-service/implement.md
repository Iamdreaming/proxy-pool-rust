# Implement: standalone REST-client MCP service

STATUS: Phases 1-6 done, workspace test+clippy green. Remaining: trellis-check,
commit, push, CI, deploy, verify.

Design refinement during impl (Option Y): kept `store` + `geoip` LOCAL in the MCP
process (the bin loads the same `settings.yaml`, so ScoreWeights/min_score match
exactly). Only the 9 truly in-proc-only tools route via REST (service_status,
xray_status, warp_status, refresh_pool, fetcher_status, subscription_sources,
refresh_subscription_source, refresh_fetcher, route_test). This halves the churn
vs the all-REST variant. Single Docker image now carries both binaries; CI
unchanged. proxy_stats endpoint returns all 4 protocols directly.

Validation after each phase: `cargo test -p <crate>`; final `cargo test` +
`cargo clippy --workspace --all-targets -- -D warnings`.

## Phase 1 — proxy-api gap endpoints  [DONE]
1. [x] `POST /api/proxies/cleanup` → `store.cleanup_low_score`.
2. [x] `POST /api/proxy/{key}/mark-failed` → `store.mark_failed`.
3. [x] `GET /api/proxies/stats` → all-protocol distribution (chose a dedicated
       endpoint over mutating `/api/status.pool` semantics).
4. [ ] Route wiring in `routes.rs`; handler unit/smoke tests.

## Phase 2 — RestClient
5. [ ] `crates/proxy-mcp/Cargo.toml`: add `reqwest` (json, no default TLS churn —
       match workspace features) and a `[[bin]]` target.
6. [ ] `rest_client.rs`: `RestClient` with get/post/delete JSON, 15s timeout,
       `RestError`, pure `build_url`/`map_status` helpers + unit tests.

## Phase 3 — REST-backed MCP handler
7. [ ] Refactor `ProxyPoolMcp` struct + `ProxyPoolMcpConfig` to
       `{rest, geoip, update_status, git_hash, started_at}`.
8. [ ] Rewrite the 17 REST-routed tool bodies to call `self.rest`; preserve each
       tool's output JSON shape (map where REST body differs).
9. [ ] Keep local tools unchanged: check_proxy, check_proxy_matrix, geoip_lookup,
       update_service, update_status.
10. [ ] Move the axum `/mcp` serving block into `serve_http(mcp, port)` in
        proxy-mcp (reused by the bin).

## Phase 4 — standalone binary
11. [ ] `src/bin/proxy-mcp-server.rs`: env config, optional GeoIP build, serve_http.

## Phase 5 — main service slim-down
12. [ ] `proxy-server/main.rs`: remove MCP construction + HTTP/stdio serving +
        select arm; drop `proxy-mcp` dep from proxy-server Cargo.toml.

## Phase 6 — deployment
13. [ ] `deploy/Dockerfile.mcp` (or shared Dockerfile w/ bin arg).
14. [ ] `docker-compose.yml`: add `proxy-mcp` service (docker.sock + config
        mounts, MCP_UPSTREAM_API_URL, PROXY_POOL_UPDATE_* moved here, depends_on);
        remove `9000:9000` + update env from `proxy-pool`.
15. [ ] CI `docker-build.yml`: build/push the mcp image too.

## Phase 7 — verify + ship
16. [ ] full `cargo test` + clippy green.
17. [ ] trellis-check; commit; push; watch CI (both images).
18. [ ] deploy; confirm `.mcp.json` :9000 still works; call `update_service` from
        MCP → returns terminal result, proxy-pool git_hash changes, no hang.

## Rollback points
- Phases 1–4 are additive to proxy-mcp/proxy-api (safe to land incrementally).
- Phase 5 (main.rs MCP removal) + Phase 6 (compose) are the switch — keep them in
  one revertable commit so rollback = revert that commit + re-add 9000 mapping.
