# Subscription Source Report Contract

## Scenario: Source-Level Preview Recommendations

### 1. Scope / Trigger

- Trigger: `SubscriptionSourceReport` is serialized through API and MCP surfaces, so report shape changes are cross-layer contracts.
- Owner: `proxy-sub` owns report fields, recommendation calculation, and apply-blocking policy.
- Consumers: `proxy-api`, `proxy-mcp`, future frontend views, and operators.

### 2. Signatures

```rust
pub struct SubscriptionSourceReport {
    pub discovered_urls: usize,
    pub unique_urls: usize,
    pub duplicate_urls: usize,
    pub fetched_urls: usize,
    pub failed_urls: usize,
    pub parsed_nodes: usize,
    pub direct_nodes: usize,
    pub encrypted_nodes: usize,
    pub unknown_nodes: usize,
    pub duplicate_nodes: usize,
    pub protocol_counts: BTreeMap<String, usize>,
    pub errors: Vec<SubscriptionSourceError>,
    pub recommendation: SubscriptionApplyRecommendation,
}

#[serde(rename_all = "snake_case")]
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
```

### 3. Contracts

- `recommendation.decision` is `apply`, `review`, or `reject`.
- `recommendation.grade` is a coarse `0..=100` source-level grade for scanning.
- `recommendation.metrics` is derived from pre-apply report counters only.
- `recommendation.reasons` must be short stable reason strings, not raw upstream errors or subscription content.
- `direct_nodes` counts directly pool-usable basic proxies.
- `encrypted_nodes` counts supported encrypted nodes that can enter pending xray activation (`ss`, `vmess`, `trojan`).
- `unknown_nodes` counts unsupported or malformed protocol entries.
- `Unknown` entries must not be written to `PendingStore`.

### 4. Validation & Error Matrix

| Condition | Decision / Error |
|---|---|
| No discovered/unique URLs | `reject`, reason `no_subscription_urls_discovered` |
| No fetched URLs | `reject`, reason `no_urls_fetched` |
| No supported nodes (`direct_nodes + encrypted_nodes == 0`) | `reject`, reason `no_supported_nodes` |
| Fetch success rate below 10% | `reject`, reason `fetch_success_rate_below_10_percent` |
| Supported protocol ratio below 10% | `reject`, reason `supported_protocol_ratio_below_10_percent` |
| Unknown node ratio above 80% and fewer than 20 supported nodes | `reject`, reason `unknown_node_ratio_above_80_percent` |
| Duplicate node ratio above 95% with at least 20 parsed nodes | `reject`, reason `duplicate_node_ratio_above_95_percent` |
| Usable but below apply thresholds | `review` with the failed threshold reasons |
| Meets first-version apply thresholds | `apply`, reason `source_meets_apply_thresholds` |
| `apply=true` with `reject` recommendation | no writes; add `recommendation_policy` error |

### 5. Good/Base/Bad Cases

- Good: 30 parsed nodes, fetch success >= 60%, supported ratio >= 50%, unknown <= 40%, duplicate <= 70% -> `apply`.
- Base: 5 supported nodes from one fetched URL -> `review`; operator may apply intentionally.
- Noisy but usable: large mixed feeds with at least 20 supported `ss`, `vmess`, or `trojan` nodes and many unsupported entries -> `review`; normal apply may write only supported nodes.
- Bad: only `Unknown` nodes or no fetched URLs -> `reject`; normal apply is blocked.

### 6. Tests Required

- Unit test `apply`, `review`, and `reject` recommendation outcomes in `proxy-sub`.
- Unit test that `reject` apply policy inserts `recommendation_policy` and leaves `stored_basic` / `stored_encrypted` at zero.
- Serialization test that API responses include `recommendation`.
- Serialization test that MCP tool JSON includes `recommendation`.
- Partition test that `Unknown` entries are skipped from encrypted pending output.

### 7. Wrong vs Correct

#### Wrong

```rust
let (basic, encrypted) = partition(&proxies, url);
pending.store_batch(&encrypted).await?;
```

This is wrong if `encrypted` includes `Unknown` nodes. Unsupported entries should be visible in counters, not queued for xray activation.

#### Correct

```rust
let (basic, encrypted) = partition(&proxies, url);
report.unknown_nodes += proxies.iter().filter(|p| matches!(p, SubscriptionProxy::Unknown { .. })).count();
// partition only returns Basic conversions and supported encrypted nodes.
```

Recommendation calculation must happen before writes. A `reject` decision blocks normal apply by policy.
