# subscription-source-ops-mvp

## Goal

Expose subscription source status, manual refresh, parse preview, and structured API/MCP ops surfaces.

## Background

`proxy-sub` already discovers subscription URLs, fetches content, parses nodes,
partitions direct basic proxies from encrypted nodes, stores basic nodes in the
pool, and stores encrypted nodes in Redis pending sets. Today that refresh path
is mostly log-only: API/MCP operators cannot see which sources exist, which
sources failed, how many nodes were parsed, or manually preview a single source
without waiting for the background loop.

## Requirements

### F1: Source Status

- Expose configured subscription sources from static URLs, GitHub search, and
  aggregators.
- Each source status includes a stable id, source kind, display URL or label,
  enabled/configured state, last refresh timestamp, last outcome, last error,
  discovered URL count, parsed node counts, stored basic/encrypted counts, and
  duplicate/unknown counts where available.
- When no sources are configured, return an empty list with a clear enabled
  state instead of an error.

### F2: Manual Refresh and Preview

- Support manual refresh of one subscription source by stable id.
- Manual refresh returns a structured report with discovered/fetched URLs,
  parsed counts by protocol, dedup summary, stored counts, failures, and elapsed
  time.
- Support a dry-run/preview mode that fetches and parses but does not write to
  `ProxyStore` or `PendingStore`.
- Default manual operations must be dry-run/preview unless explicitly requested
  otherwise.

### F3: API and MCP Visibility

- REST exposes subscription status and manual refresh endpoints.
- MCP exposes matching tools for status and manual refresh/preview.
- Response shapes are shared or mirrored closely enough that integration tests
  can assert the same contract across REST and MCP.

### F4: Compatibility and Boundaries

- Keep the existing background subscription refresh loop behavior compatible.
- Do not add subscription source CRUD in this slice.
- Do not expose raw subscription content, credentials, passwords, UUIDs, or full
  node configs in API/MCP responses.
- Do not directly SSH to the dev host for validation.

## Acceptance Criteria

- [x] API can query subscription source status, including an empty configured
  state when no sources exist.
- [x] API can manually preview/refresh one configured source by id with
  structured success/failure results.
- [x] MCP can query the same status and perform the same preview/refresh action.
- [x] Manual action defaults to dry-run/preview and only writes when explicitly
  requested.
- [x] Parse results include total/direct/encrypted/unknown counts and per-source
  errors without leaking secrets.
- [x] Existing background subscription refresh still runs.
- [x] `cargo test --workspace --all-targets` passes.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` passes.

## Verification

- `cargo fmt --all --check`
- `cargo test -p proxy-sub`
- `cargo check -p proxy-api -p proxy-mcp -p proxy-server`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- No-SSH post-push smoke through GitHub Actions and public HTTP/MCP surfaces.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
