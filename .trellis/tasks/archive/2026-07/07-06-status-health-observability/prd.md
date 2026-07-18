# PRD: status-health-observability

## Goal

Make `proxy-pool-rust` observable enough to operate after automated deploys.
Operators should be able to tell whether the process is alive, whether it is
ready to serve traffic, which version/image is running, and whether core
dependencies and background services are healthy without SSHing into the host.

## Background

- The CI/CD + MCP self-update loop is now functional: push to `main` builds and
  pushes a GHCR image, and MCP `update_service` can trigger a remote update.
- Current `/api/status` returns `version`, `git_hash`, and pool counts, but it
  masks Redis count errors with `unwrap_or(0)`.
- Current `/api/metrics` exposes only pool counts and also masks Redis count
  errors.
- There is no `/api/healthz` or `/api/readyz` endpoint.
- MCP exposes pool/WARP tools, but there is no single `service_status` tool that
  mirrors API status.
- Docker Compose does not define a service healthcheck.
- Web Dashboard changes are intentionally out of scope for this task.

## Requirements

### R1: Liveness Endpoint

Add `GET /api/healthz` as a lightweight process liveness endpoint.

- It should not depend on Redis, proxy pool size, WARP, xray, or external
  network calls.
- It should return HTTP 200 when the API process is running.
- Response should be small and structured JSON, for example `{ "status": "ok" }`.

### R2: Readiness Endpoint

Add `GET /api/readyz` as an operational readiness endpoint.

- It must check Redis availability because Redis is required for pool storage.
- It should return HTTP 200 when required dependencies are healthy.
- It should return HTTP 503 with structured JSON when Redis is unavailable.
- It should avoid slow external network checks.

### R3: Expanded Status Contract

Expand `GET /api/status` into a structured service summary.

Required fields:

- `version`
- `git_hash`
- `uptime_sec`
- `pool` counts by protocol and total
- `redis` status summary
- `warp` summary, including configured/healthy counts where available
- `xray` summary, including active node count

Behavior:

- Redis errors must not be silently converted into zero counts.
- Status may still return HTTP 200 with a degraded dependency summary if the
  service process is alive, but the Redis error must be visible in the response.

### R4: Metrics Improvements

Improve `GET /api/metrics` enough to support basic monitoring.

- Preserve existing `proxy_pool_size{protocol=...}` metrics.
- Add total pool count.
- Add Redis readiness/status metric.
- Add WARP healthy/configured metrics where available.
- Add xray active node metric.
- Do not make metrics collection depend on slow external network calls.

### R5: MCP Service Status Tool

Add MCP `service_status`.

- It should return a structured JSON status aligned with `/api/status`.
- It should not duplicate business logic in `proxy-mcp`; shared status assembly
  should live in the appropriate API/core/service layer if needed.
- It should surface Redis degraded state clearly.

### R6: Docker Healthcheck

Add a Docker Compose healthcheck for `proxy-pool`.

- It should target `/api/healthz` or `/api/readyz` depending on final decision.
- It should be fast and safe to run repeatedly.
- It should not trigger proxy fetching or validation.

### R7: Tests

Add focused tests for the new contracts.

- Unit tests for new response serialization and helper behavior.
- Integration tests for `/api/healthz`, `/api/readyz`, and expanded
  `/api/status` shape.
- MCP tool list test should include `service_status`.

## Non-Goals

- No Web Dashboard UI work in this task.
- No change to proxy scoring, validation, routing, or fetcher behavior.
- No external alerting integration.
- No full deployment rollback feature.
- No expensive health checks that probe outbound proxy targets.

## Acceptance Criteria

- [ ] `GET /api/healthz` returns HTTP 200 with JSON when the API process is alive.
- [ ] `GET /api/readyz` distinguishes Redis healthy from Redis unavailable.
- [ ] `GET /api/status` includes `version`, `git_hash`, `uptime_sec`, `pool`,
      `redis`, `warp`, and `xray` sections.
- [ ] Redis count/readiness errors are visible rather than silently converted to
      zero counts.
- [ ] `GET /api/metrics` includes pool total, Redis status, WARP summary, and
      xray active-node metrics.
- [ ] MCP `service_status` returns structured JSON aligned with `/api/status`.
- [ ] `docker-compose.yml` includes a healthcheck for `proxy-pool`.
- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo test --workspace --all-targets` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `npx vue-tsc --noEmit` passes.
- [ ] `npm run build` passes.
- [ ] Relevant integration tests pass against a deployed instance or are clearly
      documented as requiring a deployed instance.

## Decision

Docker Compose healthcheck uses `/api/healthz` for Docker container health so Redis
outages do not cause Docker to restart an otherwise healthy process in a loop;
use `/api/readyz` for external readiness monitoring and deployment gates.
