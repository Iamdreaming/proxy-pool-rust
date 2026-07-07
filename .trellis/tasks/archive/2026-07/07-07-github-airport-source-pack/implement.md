# GitHub Airport Source Pack Implementation Plan

## Scope

Implement backend/API/MCP recommendation support and safe source-pack documentation. Do not add a new frontend view in v1.

## Steps

1. Add recommendation models in `crates/proxy-sub/src/ops.rs`.
   - Add `SubscriptionApplyDecision`.
   - Add `SubscriptionApplyRecommendation`.
   - Add `SubscriptionSourceQualityMetrics`.
   - Serialize decisions as `snake_case`.

2. Compute recommendations for every `SubscriptionSourceReport`.
   - Add a helper such as `recommend_apply(&SubscriptionSourceReport)`.
   - Populate `recommendation` before storing/updating the report.
   - Cover `apply`, `review`, and `reject` cases in unit tests.

3. Block normal apply for rejected sources.
   - In `run_entry`, perform fetch/parse/accounting first.
   - Compute recommendation before writes.
   - If mode is `Apply` and decision is `Reject`, skip writes, append a sanitized policy error, and return the report.
   - Verify `stored_basic == 0` and `stored_encrypted == 0` for rejected apply.

4. Add config/docs for the hybrid source pack.
   - Add commented examples under `subscription` in `config/settings.example.yaml`.
   - Document curated source lane, GitHub Search candidate lane, preview/apply commands, recommendation thresholds, and rollback in README or `docs/`.
   - Keep GitHub Search disabled by default.

5. Update API/MCP tests.
   - Assert serialized reports contain `recommendation`.
   - Assert refresh response can represent blocked rejected apply.
   - Keep existing route/tool names unchanged.

6. Validate.
   - `cargo fmt -- --check`
   - `cargo test -p proxy-sub --lib`
   - `cargo test -p proxy-api --lib`
   - `cargo test -p proxy-mcp --lib`
   - Run broader checks if touched code expands beyond these crates.

## Risk Points

- Recommendation must be source-level only; do not pretend it uses latency/success/anonymity before apply.
- Reject blocking must not partially write nodes.
- Redaction must remain intact for URL labels and errors.
- Existing clients should tolerate the additive `recommendation` field.

## Review Gate

Before `task.py start`, review:

- `prd.md`
- `design.md`
- `implement.md`

Implementation starts only after user approval.
