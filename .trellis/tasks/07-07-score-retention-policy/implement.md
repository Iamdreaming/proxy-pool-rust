# Implementation Plan: Score Retention Policy

## Phase 1: Planning

- [x] Create Trellis task.
- [x] Inspect existing score, store, config, API, MCP, and tests.
- [x] Write PRD, design, and implementation plan.

## Phase 2: Core Score Explanation

- [x] Add `ScoreComponent`, `SuccessScoreComponent`, `AnonymityScoreComponent`, `RetentionDecision`, and `ScoreExplanation`.
- [x] Add `explain_score(proxy, weights, min_score)`.
- [x] Keep `score(proxy, weights)` behavior compatible.
- [x] Add unit tests for neutral, fast elite, below-min, and hard-failure cases.

## Phase 3: Store Operations

- [x] Expose store-level score explanation helpers using configured weights/min_score.
- [x] Add low-score cleanup scan/remove method with dry-run-friendly result shape.
- [x] Add unit tests for cleanup eligibility using in-memory/proxy-level logic where Redis is not required.

## Phase 4: API

- [x] Add `GET /api/proxies/scores`.
- [x] Reuse existing filter params.
- [x] Add response serialization tests.

## Phase 5: MCP

- [x] Add `explain_proxy_scores` tool.
- [x] Add `cleanup_low_score_proxies` with `apply` defaulting to false.
- [x] Add parameter deserialization tests.

## Phase 6: Documentation

- [x] Add score formula and retention rules to docs.
- [x] Update Roadmap status when complete.

## Phase 7: Verification

- [x] `cargo fmt --all --check`
- [x] `cargo test -p proxy-core --lib`
- [x] `cargo test -p proxy-api --lib`
- [x] `cargo test -p proxy-mcp --lib`
- [x] `cargo test --workspace --all-targets`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`

## Risk Points

- Destructive cleanup must remain opt-in through `apply: true`.
- Score math must not change silently while adding explanation fields.
- Redis cleanup must remove exact stored members by proxy identity, not by string score alone.
