# Design: 高质量海外稳定代理获取（Parent）

## Architecture Boundary

Parent 不新增 crate。能力落在既有边界：

| Layer | Crate | Responsibility under this program |
|-------|-------|-----------------------------------|
| Fetch / score / store / validate | `proxy-core` | 多目标准入、评分曲线、retention、cleanup、pool filters |
| Subscription parse / recommend | `proxy-sub` + core subscription ops | URL intake、preview/apply、源级 recommendation |
| Encrypted node runtime | `proxy-xray` | 激活、admission 验证、active/failed 生命周期 |
| WARP | `proxy-core` warp | healthy 实例作 overseas fallback |
| Route selection | `proxy-gateway` + `proxy-core` route_debug | 海外 stable：xray 优先，WARP fallback |
| Ops surface | `proxy-api` / `proxy-mcp` | status、scores、cleanup、subscription refresh、xray status |

## Target Routing Semantics (D3 + D4)

```text
overseas request
    │
    ├─ if active xray nodes meeting D1/D2 ≥ 1 (prefer ≥ 3)
    │     → pick xray upstream
    │
    ├─ else if WARP healthy
    │     → WARP fallback
    │
    └─ else
          → fail closed or optional explicit extended (free) path
             (NOT default stable)
```

Free/basic proxies:

- remain in protocol Redis sets as today
- may be listed/filtered via API/MCP
- **never** labeled or default-selected as stable overseas

## Overseas Validation Profile (D1 + D2)

Shared target list (conceptually one profile, config shape owned by admission child):

1. `https://www.cloudflare.com/cdn-cgi/trace`
2. `https://api.ipify.org`
3. `https://www.youtube.com`

Rules:

- timeout per target: **5s**
- overseas/stable admission: **all targets pass**
- xray should use the same profile (or explicit `xray.validate_targets` equal to it)
- pool single-URL default can remain for backward compatible free validation until admission child migrates defaults carefully

## Scoring Corrections (owned by quality-admission-scoring)

Problems to fix without rewriting store schema:

1. Latency norm saturates at 2s → >2s nodes tied on latency contribution.
2. `min_score=0.1` retains almost everything.
3. trend is explain-only.

Design direction:

- Replace or extend latency curve so 5s/10s score materially worse than 1s.
- Raise recommended/overseas min_score to **0.35** (global default change needs compatibility note in child design).
- Prefer feeding recent success / p50 into score or hard retention gates.
- Keep Redis sorted-set score as single rank key unless child proves a second index is required.

## Subscription / Trial Intake (owned by trial-sub-intake-workflow)

Reuse existing subscription pipeline:

```text
operator pastes URL → config or ops API
    → preview refresh (apply=false)
    → recommendation grade
    → apply=true only when apply/review accepted by operator
    → encrypted pending → xray sync/admission
    → observe xray_status + quality
```

Hard non-goal: any browser/email/SMS/CAPTCHA registration automation.

## Observability

Reuse, do not duplicate:

- `service_status.quality` buckets + recent_success_rate
- `explain_proxy_scores` + trend
- `fetcher_status` / `subscription_sources`
- `xray_status` / `warp_status`
- `cleanup_low_score_proxies` dry-run/apply

Parent integration success = these surfaces show **stable overseas signal** (xray active ≥ 3 or WARP fallback healthy), not larger free totals.

## Compatibility

- Existing free fetchers stay; priority and retention change.
- Existing MCP/API tools stay; new defaults/docs may recommend filters.
- `07-08-vless-xray-validation` remains the primary implementation vehicle for VLESS + xray target validation; subscription-xray child integrates and fills gaps only.

## Rollback Shape

| Change class | Rollback |
|--------------|----------|
| Config timeouts / targets / min_score | revert yaml |
| Score formula | feature flag or versioned formula + redeploy |
| Gateway stable preference | config route policy revert to previous selection |
| Cleanup apply | restore from Redis backup if any; prefer dry-run first |
| Subscription apply | disable source id; cleanup low-score / failed xray nodes |

## Trade-offs

| Choice | Benefit | Cost |
|--------|---------|------|
| Stable = xray+WARP only | Clear SLA story | Fewer “stable” nodes until xray works |
| Strict 3-target admission | Real overseas usability | Lower admit rate on public subs |
| No auto-registration | Legal/ToS safe | Operator must paste trial URLs manually |
| xray before WARP | Saves WARP for true fallback | When xray=0, depends on WARP immediately |
