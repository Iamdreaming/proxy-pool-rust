# PRD: Score Retention Policy

## Background

The proxy pool already stores proxies in Redis sorted sets using a composite score. Operators can filter by `min_score`, and `mark_failed` can evict proxies below the store-level threshold. However, the score is opaque: API/MCP clients cannot see why a proxy scored well or poorly, and there is no safe operations entry point for reviewing or cleaning low-quality proxies.

## Goal

Make proxy scoring explainable and provide a safe manual cleanup path for low-score proxies without adding an always-on destructive background job in the first slice.

## Confirmed Facts

- `proxy_core::store::score(proxy, weights)` computes score from latency, success/failure counters, and anonymity.
- `PoolSettings` exposes `min_score` and `score_weights` with defaults: latency `0.5`, success `0.3`, anonymity `0.2`, min score `0.1`.
- `ProxyStore::mark_failed` and `mark_failed_with_circuit` already evict proxies when the recalculated score is below store `min_score` or hard failure threshold is exceeded.
- API and MCP filter params already accept `min_score`, but responses return raw proxies without score explanation.
- MCP has `remove_proxy`, `proxy_stats`, `list_proxies`, and `get_best_proxy`, but no `cleanup_low_score_proxies`.

## Requirements

### F1: Score Explanation Model

- Add a serializable score explanation model in `proxy-core`.
- The model must include at least:
  - final `score`
  - latency component: raw latency, normalized value, weight, contribution
  - success component: success count, fail count, success rate, weight, contribution
  - anonymity component: raw anonymity, normalized value, weight, contribution
  - `min_score`
  - retention decision: keep, below_min_score, or hard_failure_evict
- Existing `score(proxy, weights)` behavior must remain compatible.

### F2: API Score Explain Surface

- Add a REST endpoint that can return score explanation for stored proxies.
- It must support `protocol`, optional `limit`, and the same filter fields used by `/api/proxies`.
- Response must include proxy + score explanation pairs.

### F3: MCP Score Explain Surface

- Add an MCP tool that returns score explanation for stored proxies.
- It must support protocol, limit, and existing filter fields.
- Output must be structured JSON.

### F4: Safe Low-Score Cleanup

- Add MCP `cleanup_low_score_proxies` with an explicit safety switch.
- Default behavior must be dry-run and non-destructive.
- Destructive cleanup must require an explicit boolean flag such as `apply: true`.
- The tool must report scanned, eligible, removed, and per-proxy score explanation.
- The first slice does not add a periodic background cleanup job.

### F5: Documentation And Verification

- Document the current score formula and retention rules.
- Unit tests must cover explanation math and retention decision boundaries.
- API/MCP serialization or parameter tests must cover the new operations surface.

## Non-Goals

- No background cleanup scheduler in this slice.
- No Redis schema migration.
- No change to the existing score formula unless required to expose its components.
- No per-protocol `min_score` config in this slice; document it as a follow-up if the current config shape cannot support it safely.
- No direct SSH dev validation.

## Acceptance Criteria

1. `proxy-core` can produce a stable JSON-serializable score explanation for a proxy.
2. REST API can return score explanations for stored proxies.
3. MCP can return score explanations for stored proxies.
4. MCP `cleanup_low_score_proxies` is dry-run by default and requires `apply: true` to remove proxies.
5. Documentation states the score formula, default weights, min score behavior, and cleanup safety behavior.
6. Relevant Rust tests pass, including `cargo test -p proxy-core --lib`, `cargo test -p proxy-api --lib`, and `cargo test -p proxy-mcp --lib`.
