# PRD: Gateway Route Debugging

## Background

The deployment and baseline status loop are in place, and the roadmap now puts gateway route debugging as the next active work item. The gateway is the runtime path users actually depend on, but today it is hard to answer why a request used direct, pool proxy, WARP, xray, or ended as 502.

## Goal

Make gateway routing decisions and fallback behavior explainable, observable, and testable without changing the public proxy protocols.

## Confirmed Facts

- `crates/proxy-core/src/router.rs` maps host suffixes to routing groups and only exposes `match_group(host) -> &str`.
- `crates/proxy-gateway/src/upstream.rs` owns `UpstreamSelector::select(host, protocol) -> Upstream`.
- `UpstreamSelector::select` returns only the final upstream choice, so callers cannot inspect matched route rule, GeoIP result, skipped exits, or fallback order.
- `crates/proxy-gateway/src/http_connect.rs` and `crates/proxy-gateway/src/socks5.rs` return 502 / SOCKS failure after the selected upstream connection fails; they do not currently retry the next fallback candidate.
- `crates/proxy-api/src/routes.rs` exposes status, metrics, proxy, fetcher, WARP, and xray endpoints, but no route dry-run endpoint.
- `crates/proxy-mcp/src/lib.rs` exposes operational tools, but no `route_test` tool.
- `config/routes.example.yaml` defines route groups `direct`, `warp`, and `free_pool`, with `direct` as the default group.
- Direct SSH to the dev address is not allowed; validation must use local tests plus HTTP/MCP/GitHub Actions/update_service paths when deployment is involved.

## Requirements

### F1: Structured Route Decision Model

- Add a serializable route decision model that can explain:
  - input host and protocol
  - matched route group
  - matched rule or reason when available
  - GeoIP country and overseas decision when GeoIP is consulted
  - ordered candidate exits considered for the request
  - final selected exit
  - skipped or unavailable exits with reasons
- Keep the existing `Upstream` runtime type usable for proxy handlers.

### F2: Route Dry-Run

- Add a dry-run path that evaluates the decision chain without opening a client tunnel to the target.
- Dry-run input must support at least `host` and optional `protocol`.
- Dry-run output must be deterministic enough for API/MCP clients and tests to assert stable fields.

### F3: Real Gateway Fallback Trace

- HTTP CONNECT and SOCKS5 handlers must record the selected exit and connection outcome.
- When a configured fallback candidate is unavailable or fails to connect, the trace must show which candidate failed and why.
- If implementation changes runtime fallback behavior to try the next candidate after connection failure, it must preserve protocol responses: HTTP returns 200 only after a tunnel is established, otherwise 502; SOCKS5 returns success only after a tunnel is established, otherwise a failure reply.

### F4: API And MCP Operations Surface

- REST API must expose a route dry-run endpoint.
- MCP must expose a `route_test` tool.
- Responses must contain structured JSON, not log-only text.
- Unknown or malformed input must return structured errors.

### F5: Metrics

- Prometheus metrics must distinguish gateway route outcomes by exit type at minimum:
  - `direct`
  - `free_pool`
  - `warp`
  - `xray`
  - `no_proxy`
- Metrics should include success/failure counts or fallback attempt counts if the existing architecture can support them without a new metrics subsystem.

### F6: Tests And Validation

- Unit tests must cover route decision construction for direct route, explicit proxy group, GeoIP-assisted default route, and no-upstream cases.
- Adapter tests must cover API/MCP serialization for route dry-run.
- Gateway tests must cover at least direct success and no-upstream failure; fallback connection retry tests are required if runtime retry behavior is implemented in this task.

## Non-Goals

- No Web Dashboard work in this task.
- No route file format redesign beyond optional additive fields needed for explainability.
- No Redis schema migration.
- No authentication or access-control work.
- No direct SSH-based dev validation.
- No WARP/xray lifecycle management changes beyond reading their availability for route decisions.

## Acceptance Criteria

1. `route_test` via MCP returns the target, matched group/reason, candidate exits, final decision, and unavailable reasons for representative hosts.
2. REST route dry-run returns equivalent structured data.
3. Gateway request handling emits structured logs or metrics that identify final exit type and failure outcome.
4. Prometheus metrics expose gateway route/fallback counters or gauges with stable names and labels.
5. `cargo test -p proxy-core --lib` passes for router/decision model behavior.
6. `cargo test -p proxy-gateway --lib` passes for selector and gateway behavior.
7. `cargo test -p proxy-api --lib` passes for route response serialization.
8. `cargo test -p proxy-mcp --lib` passes for `route_test` params/serialization.
9. `cargo test --workspace --all-targets` passes.
10. `cargo clippy --workspace --all-targets -- -D warnings` passes.

## Open Questions

None blocking for planning. The recommended first implementation keeps route dry-run and trace data in memory/process metrics only, with persistent history deferred.
