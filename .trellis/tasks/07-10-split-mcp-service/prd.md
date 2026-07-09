# Split MCP server into standalone REST-client service

## Goal

Run the MCP server as its own process/container, decoupled from the main
`proxy-pool` service, so that triggering `update_service` (which restarts the
main container via Watchtower) no longer kills the process serving the MCP
response. Today MCP is embedded in `proxy-server` and shares in-process `Arc`
state; the update call hangs indefinitely because the responder is the very
container being recreated.

## Background (verified by research)

22 `#[tool]` methods in `crates/proxy-mcp/src/lib.rs`. Dependency classes:
- **store (Redis-backed)** — get_proxy, get_best_proxy, list_proxies,
  explain_proxy_scores, cleanup_low_score_proxies, remove_proxy, pool_status,
  proxy_stats.
- **in-proc-only handles** (scheduler_handle mpsc, route_selector metrics,
  xray_status registry, balancer, subscription_ops status) — service_status,
  xray_status, warp_status, refresh_pool, fetcher_status, subscription_sources,
  refresh_subscription_source, refresh_fetcher, route_test.
- **pure network / local** — check_proxy, check_proxy_matrix, geoip_lookup.
- **Docker/Watchtower + MCP-local state** — update_service, update_status.

REST API (`proxy-api`) already exposes matching endpoints for every in-proc-only
tool. Only two true gaps need new endpoints: `cleanup_low_score_proxies` and the
`remove_proxy` mark-failed semantics (DELETE currently does plain remove).

## Requirements

R1. A standalone MCP binary serving the HTTP (streamable-http) transport, built
    into its own container image.
R2. MCP tools reach the main service via REST (`/api/*`) for all in-proc-state
    tools. A single base URL is configured via env (e.g. `MCP_UPSTREAM_API_URL`,
    default `http://proxy-pool:8000`).
R3. Pure-network/local tools (check_proxy, check_proxy_matrix, geoip_lookup) run
    inside the MCP process. geoip needs the MMDB file mounted + Redis reachable.
R4. `update_service` / `update_status` live in the MCP container (Docker socket
    mount + Watchtower HTTP + `PROXY_POOL_UPDATE_*` env). `update_service` targets
    the sibling `proxy-pool` container; MCP survives its restart and returns.
R5. The main `proxy-server` stops serving the MCP HTTP transport (keeps API,
    gateway, scheduler, subscription, xray). No behavior change to those.
R6. Two new REST endpoints in `proxy-api` to close gaps without direct-Redis
    scoring-config duplication:
    - `POST /api/proxies/cleanup` → `CleanupLowScoreResult`.
    - A mark-failed removal for `remove_proxy` parity (extend delete_proxy with a
      mode, or add `POST /api/proxy/{key}/mark-failed`).
R7. Behavior parity: every one of the 22 tools returns data equivalent to today.
    `proxy_stats` must still include the Socks4 per-protocol count (add socks4 to
    the count source or compute in MCP via REST list).
R8. `deploy/docker-compose.yml`: add a `proxy-mcp` service (own container, port
    9000, `/var/run/docker.sock` + config/MMDB mounts, `depends_on: proxy-pool,
    redis`). Remove the 9000 port mapping from `proxy-pool`.

## Constraints

- Edition 2024; thiserror/anyhow; tokio; tracing; serde. No new heavy deps beyond
  `reqwest` (already in the workspace).
- One canonical set of `#[tool]` definitions — no copy-paste of 22 tools.
- REST client failures must degrade gracefully (tool returns a structured error,
  not a panic/hang); add request timeouts.

## Out of scope

- Rewriting the REST API surface beyond R6.
- Auth between MCP and the API (same trusted Docker network, as today).
- Changing MCP tool schemas/semantics (parity only).
- stdio transport for the standalone MCP (HTTP only; stdio can stay a follow-up).

## Acceptance Criteria

1. `cargo build` produces a standalone MCP binary; `cargo test` green;
   `cargo clippy --workspace --all-targets -- -D warnings` clean.
2. The MCP handler compiles against a `RestBackend` and reuses the single set of
   `#[tool]` definitions (no duplicated tool bodies).
3. Unit tests: REST backend request/response mapping for representative tools
   (status, refresh_pool, route_test, xray_status, cleanup, remove); local tools
   unchanged.
4. `deploy/docker-compose.yml` defines `proxy-mcp` as a separate container with
   the docker.sock mount; `proxy-pool` no longer serves MCP.
5. Post-deploy (observation, not unit test): calling `update_service` on the MCP
   container returns a terminal JSON result while `proxy-pool` `git_hash` changes;
   the MCP call does not hang.
6. All 22 tools exercised read-only against a running stack return parity data.
