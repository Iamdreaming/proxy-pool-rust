# GitHub Airport Source Pack Design

## Summary

Add a hybrid public GitHub airport-source workflow on top of the existing subscription operations system.

The implementation will not create a second ingestion path. It will extend existing subscription source reports with a source-level recommendation and add a documented/preset configuration lane for:

- curated static or aggregator sources,
- bounded GitHub Search candidate discovery,
- preview-first operator review,
- blocked normal apply when preview says `reject`.

Post-apply node quality remains owned by existing validation, scoring, retention, xray status, and cleanup tools.

## Current Architecture

Relevant existing pieces:

- `proxy_core::config::SubscriptionConfig` already supports `urls`, `github`, and `aggregators`.
- `proxy_sub::ops::SubscriptionOpsHandle` owns API/MCP preview/apply behavior.
- `SubscriptionSourceReport` already contains source-level evidence: URL discovery, fetch success/failure, parsed node counts, protocol counts, duplicates, unknown nodes, sanitized errors, and outcome.
- `ProxyStore` scores stored proxies after validation; these node-level signals are not available before apply.
- xray activation handles encrypted `ss`, `vmess`, and `trojan` nodes after they are stored in Redis pending sets.

## Recommendation Model

Add a serializable recommendation object to `SubscriptionSourceReport`:

```rust
pub enum SubscriptionApplyDecision {
    Apply,
    Review,
    Reject,
}

pub struct SubscriptionApplyRecommendation {
    pub decision: SubscriptionApplyDecision,
    pub grade: u8,
    pub reasons: Vec<String>,
    pub metrics: SubscriptionSourceQualityMetrics,
}

pub struct SubscriptionSourceQualityMetrics {
    pub fetch_success_rate: Option<f64>,
    pub supported_protocol_ratio: Option<f64>,
    pub unknown_node_ratio: Option<f64>,
    pub duplicate_node_ratio: Option<f64>,
    pub parsed_nodes_per_url: Option<f64>,
}
```

The recommendation is computed from preview/apply report counters only. This keeps the result deterministic and available before writes.

Suggested first-version thresholds:

- `apply`: fetch success rate >= 0.60, parsed nodes >= 20, supported protocol ratio >= 0.50, unknown node ratio <= 0.40, duplicate node ratio <= 0.70, and no dominant error pattern.
- `review`: at least one usable parsed node exists, but one or more `apply` thresholds are weak.
- `reject`: no usable nodes, near-total fetch failure, extremely low supported ratio, unknown nodes dominate, extreme duplicates, or source appears malformed/private/unusable.

`grade` is a coarse 0-100 source quality grade for scanning. Reasons should be short stable strings suitable for API/MCP/CLI display.

## Apply Blocking

Normal `apply=true` must run a preview-quality evaluation before writing.

If the recommendation is `reject`, the refresh request returns a report in apply mode with no writes and an error/reason explaining that the source was blocked by recommendation policy.

No `force=true` override is included in v1. That keeps the public-source lane conservative and avoids expanding API/MCP contracts before operators need it.

## Source Pack Shape

The source pack should be configuration/documentation-first:

- add commented example entries to `config/settings.example.yaml`,
- document a recommended safe rollout path in README or a small docs page,
- include GitHub Search as disabled-by-default advanced candidate discovery with explicit keywords and low `max_results`.

The implementation should avoid hardcoding private source URLs or secrets. Public examples are acceptable as commented samples or docs references, but production config should require operator opt-in.

## API And MCP Impact

Because API and MCP serialize `SubscriptionSourceReport`, adding `recommendation` there automatically exposes it through:

- `GET /api/subscriptions/sources`,
- `POST /api/subscriptions/sources/{id}/refresh`,
- MCP `subscription_sources`,
- MCP `refresh_subscription_source`.

Update API/MCP tests to assert the field serializes and reject-block behavior is visible.

## Frontend Scope

V1 does not add a new web subscriptions page. The current web app has no subscription-source route or typed API wrappers for those endpoints. Adding a UI would make this task broader than the source-pack and recommendation contract.

The backend response shape should remain frontend-friendly so a later web view can render recommendation badges, metrics, and apply/reject actions.

## Errors And Safety

- URL query strings and fragments must stay redacted in labels and errors.
- Recommendation reasons must not include raw proxy addresses, subscription contents, tokens, or unbounded upstream error text.
- `reject` blocking must not write basic proxies to `ProxyStore` or encrypted nodes to `PendingStore`.
- `review` remains applyable because public sources may be noisy but still useful.

## Rollback

Rollback is config-first:

- remove/comment the source-pack entries under `subscription`,
- disable `subscription.github.enabled`,
- use existing low-score cleanup dry-run/apply if a noisy source was already applied,
- stop xray sync or remove pending source-derived nodes only if a later task adds source-specific cleanup.
