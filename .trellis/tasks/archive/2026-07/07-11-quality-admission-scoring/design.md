# Design: 准入收紧与评分修正

## Scope

Crate: `proxy-core`（store.rs, config.rs, validator.rs, scheduler.rs, models.rs, status.rs）  
Doc: `docs/score-retention.md`  
Config: `config/settings.example.yaml`

## F1 — Overseas / Strict Admission Profile

### Current state

- `PoolSettings.effective_validate_targets()` returns structured targets from config.
- `scheduler::validate_candidates()` calls `validate_many_against_targets()` with **`TargetAdmission::Quorum`** hardcoded.
- `TargetAdmission::Strict` already exists in `validator.rs` but is not wired to config.

### Design

1. Add `target_admission` field to `PoolSettings`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetAdmissionConfig {
    /// At least one target passing admits the proxy.
    Quorum,
    /// Every target must pass for the proxy to be admitted.
    Strict,
}

impl Default for TargetAdmissionConfig {
    fn default() -> Self { Self::Quorum }
}
```

2. `PoolSettings` gets `#[serde(default)] pub target_admission: TargetAdmissionConfig`.

3. `scheduler::validate_candidates()` reads `self.settings.target_admission` instead of hardcoding `Quorum`.

4. Config example adds:

```yaml
pool:
  # When validate_targets has multiple entries, admission policy:
  #   quorum — any target passing admits the proxy (default, backward compatible)
  #   strict — all targets must pass (recommended for overseas/stable profile)
  target_admission: quorum
```

5. **Overseas profile recommendation** (docs, not code default change): when using D1 targets, set `target_admission: strict`.

### Compatibility

- Default remains `Quorum` → no behavior change for existing deployments.
- Operators opt into `Strict` when they configure multi-target overseas validation.

## F2 — Latency Scoring Fix

### Current formula

```text
latency_norm = clamp((2000 - latency_ms) / 2000, 0, 1)
```

Problem: >2s all map to 0.0, so 2.5s and 11s are indistinguishable.

### New formula: piecewise linear with extended tail

```text
if latency_ms <= 1000:
    latency_norm = 1.0                          // excellent: <1s
elif latency_ms <= 2000:
    latency_norm = 1.0 - 0.5 * (ms - 1000) / 1000  // 1.0→0.5
elif latency_ms <= 5000:
    latency_norm = 0.5 - 0.4 * (ms - 2000) / 3000  // 0.5→0.1
elif latency_ms <= 10000:
    latency_norm = 0.1 - 0.1 * (ms - 5000) / 5000  // 0.1→0.0
else:
    latency_norm = 0.0
```

| Latency | Old norm | New norm | Effect |
|---------|----------|----------|--------|
| 500ms | 0.75 | **1.0** | excellent tier |
| 1000ms | 0.5 | **1.0** | still excellent |
| 1500ms | 0.25 | **0.75** | good |
| 2000ms | 0.0 | **0.5** | fair (was 0!) |
| 3000ms | 0.0 | **0.37** | below fair |
| 5000ms | 0.0 | **0.1** | poor but distinguishable |
| 10000ms | 0.0 | **0.0** | dead |

Key property: **1s < 2s < 5s < 10s** all produce strictly decreasing scores.

### Implementation

- Replace the single line in `score_parts()` with the piecewise function.
- Add a `latency_curve` module or inline helper with clear comments.
- Unknown latency (5000ms default) maps to 0.1 (was 0.0) — slightly more forgiving for untested but not rewarding.

### Score impact example (elite, 5/5 success)

| Latency | Old score | New score |
|---------|-----------|-----------|
| 500ms | 0.5*0.75 + 0.3*1.0 + 0.2*1.0 = **0.875** | 0.5*1.0 + 0.3 + 0.2 = **1.0** |
| 2000ms | 0.5*0 + 0.3 + 0.2 = **0.5** | 0.5*0.5 + 0.3 + 0.2 = **0.75** |
| 5000ms | 0.5*0 + 0.3 + 0.2 = **0.5** | 0.5*0.1 + 0.3 + 0.2 = **0.55** |
| 11000ms | 0.5*0 + 0.3 + 0.2 = **0.5** | 0.5*0 + 0.3 + 0.2 = **0.5** |

The 11s node is no longer tied with the 2s node. The 500ms node clearly wins.

## F3 — min_score / Retention

### Current

- `default_min_score() = 0.1`
- Hard failure evict: `fail_count > max(8, success_count * 3)`

### Design

1. **Do not change the global default** from 0.1 in code (backward compat).
2. Add a **recommended overseas min_score** constant and doc:

```rust
/// Recommended minimum score for overseas/stable proxy filtering.
/// Not a code default — set in config or API/MCP filter params.
pub const RECOMMENDED_OVERSEAS_MIN_SCORE: f64 = 0.35;
```

3. `docs/score-retention.md` and `config/settings.example.yaml` document the recommendation.
4. `explain_proxy_scores` already surfaces `min_score` — no change needed.
5. Optional: add `recent_trend` as a **soft retention signal** — if `recent_success_rate < 0.3` and `recent_samples >= 5`, flag as `trend_poor` in retention decision (informational, not eviction). This can be a separate PR if scope creeps.

### Trend → score (optional, defer if complex)

- Feeding `recent_success_rate` into the score formula would change sorted-set rankings for all existing proxies.
- Safer first step: trend as retention signal only (informational), not score component.
- If later desired, add `trend_weight` to `ScoreWeights` with default 0.0.

## F4 — Explainability

- `explain_proxy_scores` already returns `ScoreExplanation` with latency/success/anonymity components.
- After formula change, the `latency.normalized` and `latency.contribution` values will differ — this is correct and expected.
- No schema change needed; the field semantics remain the same.

## Config changes summary

```yaml
pool:
  validate_timeout_sec: 15          # unchanged default
  validate_target_url: "..."        # unchanged
  # validate_targets:               # already exists
  #   - url: "https://www.cloudflare.com/cdn-cgi/trace"
  #   - url: "https://api.ipify.org"
  #   - url: "https://www.youtube.com"
  target_admission: quorum          # NEW, default quorum
  min_score: 0.1                    # unchanged default; doc recommends 0.35 for overseas
  score_weights:                    # unchanged defaults
    latency: 0.5
    success: 0.3
    anonymity: 0.2
```

## Test plan

1. `score_parts` unit tests: verify 500ms > 1s > 2s > 5s > 10s ordering.
2. `TargetAdmissionConfig` serde round-trip.
3. Scheduler reads `target_admission` from config (integration-style test).
4. `explain_proxy_scores` returns correct new latency norms.
5. Existing tests updated where old norm values were asserted.

## Rollback

- Latency formula: feature flag not needed; the function is pure and tested. Revert commit if issues.
- `target_admission`: default is `Quorum`; reverting config or removing the field restores old behavior.
- `min_score`: config-only; revert yaml.

## Trade-offs

| Choice | Benefit | Cost |
|--------|---------|------|
| Piecewise latency curve | Distinguishes slow from very slow | Slightly more complex formula |
| `target_admission` config field | Operators choose strict vs quorum | One more config knob |
| Keep min_score default at 0.1 | No surprise for existing deployments | Operators must read docs to raise it |
| Trend as informational only | No score disruption | Trend doesn't affect ranking yet |
