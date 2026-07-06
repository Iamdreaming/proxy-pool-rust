# Design: proxy-quality-history-lite

## Approach

Use a compact rolling summary owned by `proxy-core` and embedded in the stored
`Proxy` model. This is preferable to a separate Redis event stream because the
current score and explanation path already reads full proxy JSON from protocol
sorted sets, and the first slice only needs lightweight trend fields.

Alternatives considered:

- Existing aggregate counters only: no schema change, but cannot represent
  recent trend or latency distribution.
- Per-check Redis event list: more flexible, but adds new keys, retention
  policy, migration questions, and more operational surface.
- Embedded rolling summary: additive, self-contained, and enough for the
  current operator question.

## Data Contract

Add a `QualityHistory` struct in `proxy-core` with a small bounded list of
recent samples. Each sample records:

- `checked_at`: Unix timestamp seconds.
- `success`: boolean.
- `latency_ms`: optional rounded latency for successful checks.
- `error`: optional stable failure category or reason.

The stored list should be bounded by a small constant, such as 10 samples. Older
stored proxies deserialize with an empty default history.

`ScoreExplanation` gains a `trend` object rather than scattering trend fields at
the top level. The object includes:

- `recent_samples`
- `recent_success_rate`
- `recent_latency_p50`
- `recent_failures`
- `last_checked_at_unix_secs`

## Store Flow

`ProxyStore::mark_success` appends a successful sample after updating the proxy's
existing counters and latency. `mark_failed` and `mark_failed_with_circuit`
append a failed sample before deciding whether to evict the proxy. If the proxy
is evicted, no visible score explanation remains, which matches current
retention semantics.

The `score()` function stays unchanged so Redis sorted-set scores remain
compatible. `explain_score()` reads the embedded summary and derives trend
fields for REST and MCP.

## API/MCP Impact

No new endpoints or tools are needed. Existing `/api/proxies/scores` and MCP
`explain_proxy_scores` serialize `ScoredProxy` from `proxy-core`.

## Rollout And Compatibility

This is an additive JSON field on stored proxy records. Older records default to
empty history. No destructive migration or backfill is required.

## Validation

- `cargo test -p proxy-core`
- `cargo test -p proxy-api`
- `cargo test -p proxy-mcp`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
