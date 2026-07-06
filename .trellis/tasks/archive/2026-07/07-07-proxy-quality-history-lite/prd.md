# PRD: proxy-quality-history-lite

## Background

`score-retention-policy` made current proxy scores explainable through
`/api/proxies/scores` and MCP `explain_proxy_scores`. The score still mostly
reflects current aggregate counters and latest latency, so operators cannot
distinguish a short fluctuation from a sustained quality decline.

## Goal

Record lightweight per-proxy quality trend data and expose read-only trend
fields through the existing score explanation contract.

## Non-Goals

- Do not add automatic cleanup, deletion, or apply actions.
- Do not restore paused Dashboard, contract-smoke, update-failure, WARP, or xray
  tasks.
- Do not require direct SSH or host Docker access for verification.
- Do not introduce a destructive Redis migration.

## Requirements

### F1: Lightweight history model

- Keep a compact recent-quality summary for each stored proxy.
- Track enough data to report recent success rate, recent latency p50, recent
  failure count, sample count, and last checked Unix timestamp.
- Prefer additive model fields and backward-compatible deserialization.

### F2: Store integration

- Update quality history when stored proxies are marked success or failure.
- Preserve existing sorted-set score semantics.
- Keep `score()` numerically compatible for existing Redis ordering.

### F3: Read-only score explanation

- Add trend fields to `ScoreExplanation`.
- Ensure REST `/api/proxies/scores` and MCP `explain_proxy_scores` receive the
  same structure from `proxy-core`; adapters must not recompute trend fields.

### F4: Verification

- Add focused core tests for history updates, rolling limits, p50 latency, and
  serialization.
- Add API/MCP serialization or contract tests where the public JSON shape
  changes.

## Acceptance Criteria

- [x] `Proxy` or a core-owned companion model exposes a backward-compatible
  quality history summary.
- [x] `mark_success`, `mark_failed`, and `mark_failed_with_circuit` update the
  quality history consistently.
- [x] `ScoreExplanation` includes trend fields such as
  `recent_success_rate`, `recent_latency_p50`, `recent_failures`, and
  `recent_samples`.
- [x] API `/api/proxies/scores` and MCP `explain_proxy_scores` serialize the
  new trend fields without adapter-side recomputation.
- [x] Redis schema changes are additive and tolerate older stored proxy JSON.
- [x] Local tests cover core behavior and changed API/MCP response contracts.
- [x] Verification uses local tests and public HTTP/MCP surfaces only; no direct
  SSH and no `update_service`.

## Verification

- `cargo fmt --all --check`
- `cargo test -p proxy-core`
- `cargo test -p proxy-api`
- `cargo test -p proxy-mcp`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `python -m py_compile tests\integration\test_l2_api.py tests\integration\test_l4_mcp.py`
