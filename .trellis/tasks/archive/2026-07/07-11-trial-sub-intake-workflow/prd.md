# PRD: 试用订阅人工接入工作流

## Goal

让运营/用户能把**自行获取**的试用或自有机场 subscription URL 安全接入系统：录入 → preview → apply → 验证报告。  
**明确禁止**自动注册、验证码/邮箱/手机绕过、批量账号工厂。

## Parent decisions

- Compliance boundary hard-ban on registration automation
- Supply priority: subscription/xray main path
- Intake is how trial bandwidth enters the system legally

## Confirmed facts

- Subscription sources already support static URLs, preview recommendation, apply flag.
- Docs: `docs/subscription-source-packs.md`
- MCP: `subscription_sources`, `refresh_subscription_source`

## Requirements

### F1 — Operator intake path

- Documented + operable path to add one or many subscription URLs (config and/or API/MCP).
- Preview-first default; apply is explicit.

### F2 — Quality gate after apply

- After apply, operator can see: parse counts, xray activation progress, failures, whether D1/D2 overseas profile is met.
- Bad sources can be disabled without SSH.

### F3 — Safety

- No code path that signs up for airports, solves CAPTCHA, or harvests trials.
- Reject/review recommendations continue to block reckless apply where already implemented.

### F4 — UX for humans (minimal)

- Prefer improving docs + existing API/MCP over new Dashboard.
- Optional: batch URL paste helper if low-cost; not required if config+refresh enough.

## Out of Scope

- Auto registration / account farming.
- Payment automation.
- Guaranteeing trial quota longevity.

## Depends on

- Prefer after subscription-xray path can activate nodes; otherwise intake only increases failed_nodes.

## Acceptance Criteria

- [ ] End-to-end doc: paste URL → preview → apply → observe xray/quality.
- [ ] MCP/API path works without SSH.
- [ ] Repository contains **zero** new auto-registration modules/scripts.
- [ ] Failure/disable path documented.
