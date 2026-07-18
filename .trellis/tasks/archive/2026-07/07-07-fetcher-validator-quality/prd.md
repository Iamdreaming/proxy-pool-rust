# PRD: Fetcher Validator Quality

## Background

The deployment and baseline observability loop is now in place. The next bottleneck is proxy input quality: operators can see total pool counts, but cannot tell which fetcher is stale, failing, producing malformed entries, or wasting validation capacity. MCP `check_proxy` also returns only `alive: false` on failure, which is hard for LLM/ops clients to act on.

## Goal

Improve proxy source quality diagnostics and validation explainability without rewriting the fetcher framework or changing gateway routing behavior.

## Confirmed Facts

- `proxy-core::fetcher::Fetcher` currently exposes `fetch() -> Vec<Proxy>` and treats source failures as log-and-empty.
- `SchedulerResult` currently contains only aggregate `fetched`, `validated`, `stored`, and `errors` counts.
- `proxy-api` and `proxy-mcp` already receive `SchedulerHandle`, so scheduler-owned state can be surfaced through that handle.
- `proxy-mcp::check_proxy` currently builds a one-off `Validator` and returns only alive/latency/anonymity for success or host/port/protocol for failure.
- Existing `status-health-observability` endpoints and MCP `service_status` are already implemented and should not be duplicated.

## Requirements

### F1: Per-Fetcher Run Reports

- Each configured fetcher must produce a structured run report with:
  - stable fetcher id
  - display name
  - status: `never_run`, `success`, `empty`, or `error`
  - fetched/raw candidate count
  - parsed proxy count
  - optional error reason
  - last started/finished timestamps
  - duration in milliseconds
- Existing `fetch() -> Vec<Proxy>` compatibility must remain available for existing callers.

### F2: Scheduler Status Snapshot

- Scheduler must keep the latest run report for every configured fetcher.
- Manual and periodic full refreshes must update this snapshot.
- Refresh results must include the per-fetcher reports from that refresh.

### F3: Single-Fetcher Refresh

- API and MCP clients must be able to refresh one fetcher by id without refreshing every source.
- Unknown fetcher ids must return a structured error instead of silently running a full refresh.

### F4: API/MCP Operations Surface

- REST API must expose fetcher status and single-fetcher refresh.
- MCP must expose `fetcher_status` and `refresh_fetcher` tools.
- Existing `refresh_pool` behavior must remain compatible while returning richer data where possible.

### F5: Structured Proxy Check Results

- `Validator` must expose a structured check result containing:
  - alive boolean
  - protocol, host, port
  - latency/anonymity on success
  - structured error type and message on failure
- MCP `check_proxy` must return the structured result so clients can distinguish timeout, bad status, invalid proxy URL/client build, request error, and response body read issues.

## Non-Goals

- No full fetcher framework rewrite.
- No Redis schema migration for persistent fetcher history in this task.
- No Dashboard UI.
- No gateway routing or fallback behavior changes.
- No destructive dev/prod deployment fault injection.

## Deferred Roadmap Items

These remain part of the broader `fetcher-validator-quality` roadmap goal, but are not required for the first verified implementation slice:

- Source-level circuit breaker with pause and half-open probing.
- Multi-target validation URLs for default, domestic, overseas, and Cloudflare trace checks.
- TCP/request timing split, exit IP, and country/region directly in validation results.

## Acceptance Criteria

1. `cargo test -p proxy-core --lib` proves fetcher reports, scheduler status, single-fetcher selection, and validator check result behavior.
2. `cargo test -p proxy-api --lib` proves new response structs and route parameter parsing/serialization.
3. `cargo test -p proxy-mcp --lib` proves new MCP parameter structs deserialize and existing handle tests are updated.
4. `cargo test --workspace --all-targets` passes.
5. `cargo clippy --workspace --all-targets -- -D warnings` passes.
6. Roadmap is updated to reflect completed and remaining parts of `fetcher-validator-quality`.

## Open Questions

None blocking. This task will keep fetcher history in memory only; persistent history can be a later enhancement if needed.
