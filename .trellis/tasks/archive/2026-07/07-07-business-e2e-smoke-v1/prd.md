# Business Availability E2E Smoke

## Goal

Move the active roadmap toward real business usability by adding a repeatable,
no-SSH smoke path that answers two product questions:

1. Can the public gateway actually reach important target sites?
2. Do stored proxy candidates still work against those business targets, not
   only generic IP echo endpoints?

## Background

- The project already has public no-SSH deployment validation through GitHub
  Actions, `/api/status`, `/api/readyz`, MCP `service_status`, and MCP
  `update_status`.
- The core validator already records target URL, target host, HTTP status,
  timings, observed exit IP, observed country, and bounded error type.
- `/api/proxy/check-matrix` and MCP `check_proxy_matrix` already validate one
  proxy against several targets, but their request shape currently only accepts
  target URL strings. This makes targets such as OpenAI API awkward because a
  `401 Unauthorized` response proves network reachability but is treated as a
  failure.
- Existing gateway integration tests prove protocol handshakes and generic
  overseas routing, but they do not produce a business target availability
  report for OpenAI, Reddit, GitHub, and similar targets.
- Direct SSH to the dev address is not allowed. Validation must use public
  HTTP/API/MCP surfaces and local test commands.

## Requirements

1. Add a business availability smoke runner under `tests/integration/` that can
   run from the repository root using existing `PROXY_POOL_*` environment
   variables.
2. The runner must not call mutating endpoints/tools such as `refresh_pool`,
   `refresh_fetcher`, `cleanup_low_score_proxies` with apply, `remove_proxy`,
   `update_service`, or any host Docker/SSH operation.
3. The runner must test the public gateway path against a default business
   target set. The first default set is:
   - Cloudflare trace: `https://www.cloudflare.com/cdn-cgi/trace`
   - GitHub: `https://github.com/`
   - OpenAI API: `https://api.openai.com/v1/models`, expected status `401`
   - Reddit: `https://www.reddit.com/`, expected statuses `200`, `403`, `429`
4. The runner must test stored proxy candidates through the existing API by
   selecting top scored candidates and calling `/api/proxy/check-matrix`.
5. The check-matrix REST and MCP request contract must support structured
   target entries with `url` and optional `expected_statuses`, while keeping
   the existing string target list compatible.
6. A target with no configured `expected_statuses` keeps the existing success
   rule: any HTTP status below `400` is successful.
7. A target with configured `expected_statuses` is successful only when the
   response status is one of those values. This lets auth/rate-limit responses
   count as target reachability only when explicitly configured.
8. The runner must print a human-readable report and support `--json` output
   for automation.
9. The runner must precheck `/api/status.git_hash` against
   `PROXY_POOL_GIT_HASH` or local `git rev-parse --short HEAD` by default, so
   stale dev deployments are not mistaken for current-code business failures.
10. The roadmap must be updated so the active P0 direction is business
   availability, not generic metrics or broad contract smoke.

## Non-Goals

- Do not add paid proxy providers or VPN account registration automation.
- Do not automatically refresh, delete, downgrade, or mutate the proxy pool.
- Do not implement per-target routing selection in this task.
- Do not restore paused dashboard, recommendations, or broad REST/MCP contract
  smoke work.
- Do not require every public target to be reachable in all environments; this
  task creates a measurable first-layer smoke and failure report.

## Acceptance Criteria

- `tests/integration/business_e2e_smoke.py --json` can run against the
  configured public dev endpoints and returns structured results for gateway
  target checks and proxy candidate matrix checks.
- The default business target set includes OpenAI and Reddit with explicit
  expected-status handling.
- REST `/api/proxy/check-matrix` accepts both legacy string targets and
  structured targets such as
  `{"url": "https://api.openai.com/v1/models", "expected_statuses": [401]}`.
- MCP `check_proxy_matrix` accepts the same structured target shape.
- Existing legacy tests that pass `targets: ["https://example.com"]` continue
  to pass.
- Local tests cover target normalization, expected-status preservation, runner
  status classification, runtime version precheck behavior, and no-mutation
  behavior by construction.
- `docs/ROADMAP.md` marks `business-e2e-smoke-v1` as the current P0 and moves
  metrics/contract-only tasks behind the business availability line.
- The task is verified without SSH or host Docker access.
