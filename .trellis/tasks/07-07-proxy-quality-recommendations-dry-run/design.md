# Design: proxy-quality-recommendations-dry-run

## Options Considered

1. Extend `cleanup_low_score_proxies` with richer dry-run output.
   - Pro: reuses an existing MCP tool.
   - Con: the tool already has `apply`; mixing recommendations into an apply
     capable operation weakens the safety boundary.
2. Add a separate read-only recommendation helper in `proxy-core`.
   - Pro: one source of truth for API/MCP, no mutation path, easy to test.
   - Con: adds one REST route and one MCP tool.
3. Only document how to interpret `/api/proxies/scores`.
   - Pro: no code change.
   - Con: does not satisfy the operator need for concise candidates and reasons.

Choose option 2.

## Core Contract

Add recommendation structs in `proxy-core::store`:

- `ProxyQualityRecommendationResult`
- `ProxyQualityRecommendation`
- `QualityRecommendationAction`
- `QualityRecommendationSeverity`
- `QualityRecommendationReason`

Actions:

- `monitor`: weak or incomplete signal; watch before acting.
- `deprioritize`: quality is degraded enough that operators may want to avoid
  preferential use.
- `remove_candidate`: score/retention or hard-failure evidence suggests the
  proxy is a cleanup candidate.

The result is explicitly dry-run: `dry_run: true`, `applied: false`, and
`removed: 0`.

## Rule Shape

For each `ScoredProxy` from `ProxyStore::query_scored`, derive reasons:

- Retention is not `keep`: `below_min_score` or `hard_failure_evict`.
- Recent success rate is below a conservative threshold, such as `< 0.5`, when
  at least 3 recent samples exist.
- Recent latency p50 is high, such as `>= 1500ms`.
- Recent failures are high, such as at least 3 retained failures.
- Cumulative failure pressure is high, such as `fail_count >= success_count + 3`.

The highest severity reason decides the suggested action. The first slice returns
only proxies with at least one reason.

## API/MCP Impact

- REST: `GET /api/proxies/recommendations`
- MCP: `recommend_proxy_quality_actions`

Both accept protocol, limit, and the existing query filters. API/MCP do not
recompute scores or rules; they only parse params and serialize the core result.

## Compatibility

No Redis schema changes are needed. The helper reads existing stored proxies and
their additive quality history.

## Verification

- `cargo fmt --all --check`
- `cargo test -p proxy-core`
- `cargo test -p proxy-api`
- `cargo test -p proxy-mcp`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- Python integration syntax checks for changed files
