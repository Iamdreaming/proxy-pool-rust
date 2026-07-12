# PRD: 准入收紧与评分修正

## Goal

让代理“能不能进池、排第几、该不该留”反映**真实可用性**，修复当前 >2s 延迟节点仍可成为 best、min_score 过松、trend 不参与决策的问题。服务 parent `07-11-high-quality-overseas-proxy` 的 D1/D2 与 R3。

## Parent decisions this child must honor

- **D1** 三层目标：CF trace + ipify + YouTube
- **D2** 务实 SLA：单目标 5s、三层全过、p50≤2000ms、min_score 推荐 0.35、取用 max_latency 2000ms
- **D4** free 再合格也不进 stable；但本 child 的评分/准入仍作用于 free 池质量

## Confirmed facts

- Score formula documented in `docs/score-retention.md`
- Latency norm: `clamp((2000-ms)/2000,0,1)` → >2s contributes 0
- Default `pool.min_score=0.1`, `validate_timeout_sec=15`
- Multi-target config already exists (`validate_target_urls` / `validate_targets`) but not required by default
- `quality_history` / trend exist for explainability only
- Live best proxy observed ~11s latency with score ~0.5

## Requirements

### F1 — Overseas / strict admission profile

- Support configuring multi-target validation with per-target timeout (default profile aligns to D1 + 5s).
- When strict/overseas admission profile enabled, store only proxies that pass all targets (or clearly separate “candidate vs admitted” if design chooses soft path — prefer hard fail for overseas profile).
- Document how free pool validation differs from xray admission if they share helpers.

### F2 — Latency scoring fix

- High latency must rank below low latency beyond the current 2s cliff.
- Document new formula in `docs/score-retention.md`.
- Existing tests updated; add cases for 1s / 2s / 5s / 10s ordering.

### F3 — min_score / retention

- Raise recommended default or provide overseas-oriented min_score **0.35** with compatibility strategy (config change vs profile).
- Retention continues to honor hard_failure_evict.
- Optional: use trend (recent success / recent failures) as retention signal or score component.

### F4 — Explainability

- `explain_proxy_scores` remains correct under new formula.
- Operators can see why a slow proxy no longer tops the list.

## Out of Scope

- Gateway stable route policy (parent D3/D4; may be ops/xray children).
- Auto cleanup scheduler (ops child may add after dry-run comfort).
- Xray outbound generation / VLESS parsing.
- Auto registration of airport trials.

## Ordering notes

- No hard dependency on other children.
- Should land before heavy reliance on cleanup defaults and before claiming free pool “quality fixed”.

## Acceptance Criteria

- [ ] Multi-target admission profile can express D1 with 5s timeout.
- [ ] Score(1s elite success) > score(5s elite success) > score(10s elite success) with stable tests.
- [ ] Recommended min_score/path for overseas filtering is 0.35 (config and/or docs).
- [ ] `docs/score-retention.md` updated.
- [ ] `cargo test -p proxy-core` and clippy for touched crates pass.
