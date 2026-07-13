# PRD: 订阅与 xray 海外可用路径

## Goal

让订阅加密节点经 xray 激活后成为**可测的海外稳定主路径**，并在 xray 不足时明确依赖 WARP fallback（parent D3）。衔接而非重做 `07-08-vless-xray-validation`。

## Parent decisions

- D1/D2 admission targets + SLA
- D3 xray 优先，WARP fallback
- D4 stable = xray + WARP only

## Confirmed facts

- Live: encrypted nodes stored from subscriptions, but `xray active_nodes=0`, many `xray validation failed`, some config build errors (HTTP transport removed → XHTTP).
- `07-08-vless-xray-validation` already targets VLESS support + validate-before-active against external targets.
- xray can reuse `pool.effective_validate_targets()` or own `validate_targets`.

## Requirements

### F1 — Align xray admission with D1/D2

- Xray activation success means: local outbound up **and** D1 targets pass within 5s each (or documented equivalent).
- Failed nodes get readable reasons; avoid retry storms.

### F2 — Integration with in-flight VLESS task

- Inventory what `07-08-vless-xray-validation` already delivers vs gaps (transport migration errors, target list, active count SLA).
- Implement only residual gaps here; do not duplicate parsers.

### F3 — Stable overseas signal

- Target: `active_nodes >= 3` meeting D2, or explicit degraded mode when only WARP is available.
- Status surfaces (`xray_status`, `service_status`) enough for operators to see progress.

### F4 — Route preference contract

- Document/implement selection preference: xray → WARP for overseas stable (coordinate with gateway/route_debug).
- Free pool not used as stable default.

## Out of Scope

- Airport auto-registration.
- Paid providers.
- Free list expansion.

## Depends on

- Soft: parent D1/D2 (done).
- Soft: admission scoring child for shared validation helpers/docs.
- Hard: coordinate with `07-08-vless-xray-validation` worktree/branch to avoid double implementation.

## Acceptance Criteria

- [ ] Xray admission uses overseas profile targets (D1) and 5s timeout (D2).
- [ ] On a healthy subscription feed, can reach `active_nodes >= 3` **or** gap analysis explains structural feed failure with WARP fallback verified.
- [ ] Gateway/overseas route preference documented and test-covered where code changes.
- [ ] No regression to subscription preview/apply safety.
- [ ] Tests/clippy pass on touched crates.
