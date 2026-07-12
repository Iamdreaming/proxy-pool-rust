# Implement: 高质量海外稳定代理获取（Parent）

## Purpose

Parent execution plan = orchestration only. Implementation happens on children after each child’s own `prd.md` / `design.md` / `implement.md` are reviewed and that child is `task.py start`ed.

Do **not** `task.py start` this parent for coding work.

## Recommended Order

| Order | Child | Why first | Depends on |
|-------|-------|-----------|------------|
| 1 | `07-11-quality-admission-scoring` | Fixes false “best” and admission truth before expanding supply | none |
| 2 | `07-11-subscription-xray-overseas` | Main stable supply; needs clear D1/D2 targets | D1/D2 (done); ideally admission profile config |
| 3 | `07-11-ops-cleanup-pool-tiers` | Removes noise once scoring/retention meaningful | admission scoring formula/min_score |
| 4 | `07-11-trial-sub-intake-workflow` | Scales compliant supply after path works | subscription/xray path usable |

Parallel note: `07-08-vless-xray-validation` is already in_progress and should continue; child #2 must **integrate**, not fork.

## Per-child planning checklist (before any child start)

For each child:

1. Fill child `prd.md` from parent D1–D4 + child-owned requirements.
2. Write child `design.md` + `implement.md` (complex).
3. Curate `implement.jsonl` / `check.jsonl` with real specs.
4. User reviews child artifacts.
5. `python ./.trellis/scripts/task.py start <child-dir>` only for the child being implemented.

## Parent integration review (after children)

When children land, parent acceptance re-check:

1. Config documents overseas profile: 3 targets, 5s, min_score 0.35 guidance.
2. Live/dev: `xray_status.active_nodes >= 3` **or** documented WARP-only fallback mode while xray recovering.
3. `route_test` / gateway overseas path prefers xray then WARP (D3).
4. Free proxies not labeled stable (D4).
5. No auto-registration code paths introduced by trial intake.
6. `cleanup_low_score_proxies` dry-run then optional apply reduces poor/stale junk.
7. Local: `cargo test` / `clippy -D warnings` on touched crates.
8. Dev validation per `docs/dev-validation.md` (HTTP/MCP only by default).

## Immediate next action

1. User reviews parent `prd.md` + `design.md` + this file.
2. Start detailed planning on **first child**: `07-11-quality-admission-scoring` (seed its PRD from parent decisions).
3. Keep `07-08-vless-xray-validation` as the concurrent xray implementation track; parent child #2 tracks residual gaps.

## Validation commands (program-level)

```bash
cargo fmt --all --check
cargo test -p proxy-core
cargo test -p proxy-xray
cargo test -p proxy-gateway
cargo test -p proxy-api
cargo test -p proxy-mcp
cargo clippy --workspace -- -D warnings
```

Ops smoke (after deploy, no SSH):

```text
MCP service_status / xray_status / warp_status / explain_proxy_scores
MCP route_test for an overseas host
MCP cleanup_low_score_proxies apply=false
```

## Rollback points

- Prefer config-only first slices so rollback is yaml revert.
- Score formula changes need tests + docs update (`docs/score-retention.md`).
- Cleanup apply is irreversible without backup → always dry-run first.
