# Implementation Plan: Fetcher Validator Quality

## Phase 1: Planning

- [x] Read Roadmap Ready item.
- [x] Inspect current fetcher, scheduler, validator, API, and MCP code.
- [x] Create PRD, design, and implementation plan.

## Phase 2: Core Models And Fetcher Reports

- [x] Add `FetcherRunStatus`, `FetcherRunReport`, and `FetcherOutput` in `proxy-core::fetcher::base`.
- [x] Add stable `id()` to the `Fetcher` trait.
- [x] Add `fetch_with_report()` with a compatibility default.
- [x] Override built-in fetchers to report raw/parsed counts and error reasons.
- [x] Add unit tests for report status constructors and stable ids.

## Phase 3: Scheduler State And Single-Fetcher Refresh

- [x] Extend `SchedulerResult` with per-fetcher reports.
- [x] Add latest fetcher status snapshot to `Scheduler`.
- [x] Add `SchedulerHandle::fetcher_statuses()`.
- [x] Add `SchedulerHandle::refresh_fetcher(fetcher_id)`.
- [x] Add unit tests for handle behavior.

## Phase 4: API And MCP Surfaces

- [x] Add `GET /api/fetchers`.
- [x] Add `POST /api/fetchers/{id}/refresh`.
- [x] Extend API refresh response with `fetchers`.
- [x] Add MCP `fetcher_status` and `refresh_fetcher`.
- [x] Extend MCP `refresh_pool` output with fetcher reports.
- [x] Add serialization/deserialization tests.
- [x] Update deployed integration-test expectations for new API/MCP surfaces.

## Phase 5: Structured Validator Checks

- [x] Add `ProxyCheckResult` and stable error type in `proxy-core::validator`.
- [x] Make `validate_one()` delegate to `check_one()`.
- [x] Update MCP `check_proxy` to return the structured result.
- [x] Add validator unit tests around success/failure construction where network-independent.

## Phase 6: Verification

- [x] `cargo fmt --all --check`
- [x] `cargo test -p proxy-core --lib`
- [x] `cargo test -p proxy-api --lib`
- [x] `cargo test -p proxy-mcp --lib`
- [x] `cargo test --workspace --all-targets`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`

## Deferred Roadmap Items

- [ ] Source-level circuit breaker: pause repeatedly failing fetchers and probe half-open later.
- [ ] Multi-target validation URLs: default, domestic, overseas, and Cloudflare trace targets.
- [ ] Exit IP and GeoIP fields directly in validation result; current GeoIP enrichment still happens after validation in the scheduler.

## Risk Points

- Fetcher ids must stay stable because MCP/API clients will use them.
- `Fetcher::fetch()` compatibility must not recurse into `fetch_with_report()` incorrectly.
- Scheduler status is in-memory only; this is intentional for this task and should be documented as a non-goal.
- API/MCP adapters must not duplicate fetcher or validator business logic.
