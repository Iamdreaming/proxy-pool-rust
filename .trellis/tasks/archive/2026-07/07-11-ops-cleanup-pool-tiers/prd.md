# PRD: 运营清理与分池取用

## Goal

降低 free 池噪音，提供可执行的 cleanup 与取用门槛，使 API/MCP 默认推荐参数对齐 parent D2/D4：**stable ≠ free**。

## Parent decisions

- D2: min_score 0.35, max_latency 2000ms recommended
- D4: stable overseas = xray + WARP only; free is supplemental

## Confirmed facts

- `cleanup_low_score_proxies` dry-run/apply exists
- No background cleanup job yet (by design in score-retention docs)
- Filters already include min_score, max_latency, overseas, alive, source
- Live pool: many poor/stale, almost no excellent/good

## Requirements

### F1 — Cleanup playbook

- Document dry-run → inspect → apply for low-score/stale-ish proxies.
- Optional: scheduled/manual cleanup helpers only after dry-run trust (may stay manual in MVP).

### F2 — Tier semantics

- Define operator-facing tiers:
  - `stable`: xray active + WARP healthy
  - `extended` / free admitted: optional, not stable
  - `raw`: unvalidated candidates
- Prefer labeling via docs + filters over full Redis re-sharding unless necessary.

### F3 — Default fetch guidance

- Docs + examples for `get_proxy` / `get_best_proxy` with `min_score=0.35`, `max_latency=2000`, `alive=true`, `overseas=true` as appropriate.
- Ensure free best is not marketed as stable overseas.

### F4 — Disable noisy free sources

- Ability to turn off empty/error free fetchers via config (already toggle-based); document recommended disable set from live fetcher_status.

## Out of Scope

- Changing xray activation internals.
- Auto registration.
- Paying for residential APIs.

## Depends on

- Prefer after scoring child so cleanup thresholds match new scores.

## Acceptance Criteria

- [ ] Ops doc section for cleanup + recommended filters.
- [ ] Tier semantics (stable vs free) documented and reflected in status or filters.
- [ ] Dry-run cleanup returns actionable candidates; apply remains explicit.
- [ ] Tests for any new filter/tier helpers; clippy clean.
