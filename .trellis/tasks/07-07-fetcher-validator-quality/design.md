# Design: Fetcher Validator Quality

## Scope

This task adds operator-facing diagnostics around fetcher runs and proxy validation checks. The design keeps `proxy-core` as the owner of business logic, while `proxy-api` and `proxy-mcp` remain adapter layers.

## Architecture

### Fetcher Reports

`proxy-core::fetcher::base` will define:

- `FetcherRunStatus`: `never_run`, `success`, `empty`, `error`
- `FetcherRunReport`: serializable latest-run metadata
- `FetcherOutput`: internal output containing `Vec<Proxy>` plus a report

The existing `Fetcher::fetch() -> Vec<Proxy>` method remains for compatibility. A new `fetch_with_report()` default method wraps legacy fetchers, and built-in fetchers override it to include raw candidate counts and error messages.

Fetcher ids must be stable and unique. Protocol-specific fetchers use ids such as `proxyscrape:http`, `proxyscrape:https`, `thespeedx:socks5`, while display names remain human-readable.

### Scheduler State

`Scheduler` owns an `Arc<RwLock<Vec<FetcherRunReport>>>` initialized from configured fetchers as `never_run`. Each refresh updates the reports for the fetchers that were run. `SchedulerHandle` gets a clone of that shared state and exposes:

- `fetcher_statuses() -> Vec<FetcherRunReport>`
- `refresh_fetcher(fetcher_id) -> anyhow::Result<SchedulerResult>`

`SchedulerResult` is extended with `fetchers: Vec<FetcherRunReport>`. Existing aggregate fields remain unchanged.

### API Surface

New REST endpoints:

- `GET /api/fetchers` returns `{ "fetchers": [...] }`
- `POST /api/fetchers/{id}/refresh` runs one fetcher and returns the normal refresh response with `fetchers`

Existing `POST /api/proxies/refresh` keeps its aggregate fields and adds the new `fetchers` array.

### MCP Surface

New MCP tools:

- `fetcher_status`: returns the scheduler status snapshot.
- `refresh_fetcher`: accepts `{ "fetcher": "<id>" }` and returns the single-fetcher refresh result.

Existing `refresh_pool` returns the richer refresh result.

### Validator Check Results

`Validator` gains a structured `check_one()` method. `validate_one()` delegates to `check_one()` and keeps returning `Option<Proxy>` for existing scheduler flows.

Failure types are intentionally coarse and stable:

- `invalid_proxy_url`
- `client_build_failed`
- `request_failed`
- `bad_status`
- `body_read_failed`

The MCP `check_proxy` tool returns this structured result directly.

## Compatibility

- No Redis schema changes.
- No API routes are removed.
- Existing response fields remain present; new fields are additive.
- Existing fetcher source strings on stored proxies remain unchanged unless a fetcher already sets them differently.

## Error Handling

- Fetcher network and parse failures remain best-effort: the scheduler continues with other fetchers.
- Single-fetcher refresh returns an error for unknown ids; API maps it to 404 and MCP embeds `status: "error"` in JSON.
- Validator failures are returned as structured data, not MCP protocol errors.

## Rollback

Revert the task commit. Since state is in memory and no persistent data format changes are made, rollback only requires rebuilding/redeploying the previous image.
