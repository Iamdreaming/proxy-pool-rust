# Score Retention Policy

The proxy pool stores each protocol in a Redis sorted set. The sorted-set score
is the current proxy quality score.

## Formula

Current score:

```text
score =
  latency_weight   * latency_norm +
  success_weight   * success_rate +
  anonymity_weight * anonymity_norm
```

Default weights from `config/settings.example.yaml`:

| Component | Default |
|-----------|---------|
| latency | `0.5` |
| success | `0.3` |
| anonymity | `0.2` |

Latency normalization:

```text
latency_norm = clamp((2000 - latency_ms) / 2000, 0, 1)
```

Unknown latency is treated as `5000ms`, which contributes `0`.

Success rate:

```text
success_rate = 0.5                       when success_count + fail_count == 0
success_rate = clamp((success - fail) / total, 0, 1)
```

Anonymity normalization:

| Anonymity | Normalized |
|-----------|------------|
| elite | `1.0` |
| anonymous | `0.5` |
| transparent | `0.0` |
| unknown | `0.0` |

## Retention

The configured `pool.min_score` defaults to `0.1`.

When a proxy is marked failed, the store removes the existing member, increments
`fail_count`, recalculates score, and only reinserts the proxy when it still
passes retention.

Retention decisions:

| Decision | Meaning |
|----------|---------|
| `keep` | Proxy remains eligible for the pool |
| `below_min_score` | Proxy score is below the configured or supplied min score |
| `hard_failure_evict` | `fail_count > max(8, success_count * 3)` |

Hard failure eviction wins over the min-score decision.

## Explainability

Use REST `/api/proxies/scores` or MCP `explain_proxy_scores` to inspect stored
proxies with score components, recent quality trend, and retention decisions.

Each score explanation includes a `trend` object derived from the proxy's
bounded recent validation history:

| Field | Meaning |
|-------|---------|
| `recent_samples` | Number of retained recent validation observations |
| `recent_success_rate` | Successes divided by retained samples, or `null` with no samples |
| `recent_latency_p50` | Median latency from retained successful observations |
| `recent_failures` | Failed observations in the retained window |
| `last_checked_at_unix_secs` | Unix timestamp for the newest retained sample |

The trend is read-only evidence for operators. It does not change the Redis
sorted-set score formula or cleanup behavior.

## Pool Quality Summary

REST `/api/status`, MCP `service_status`, and Prometheus `/api/metrics` expose a
shared read-only pool quality summary derived in `proxy-core`.

The status `quality` object includes:

| Field | Meaning |
|-------|---------|
| `total` | Stored proxies scanned for the quality summary |
| `score_buckets` | Counts for `untested`, `poor`, `fair`, `good`, and `excellent` |
| `recent_samples` | Total retained validation observations across the pool |
| `recent_success_rate` | Aggregate recent success rate, or `null` with no recent samples |
| `recent_failures` | Total retained failed observations |
| `stale_proxies` | Proxies with no check or no check newer than `stale_after_secs` |
| `stale_after_secs` | Current stale threshold, fixed at one hour |
| `retention` | Counts for `below_min_score` and `hard_failure_evict` candidates |
| `top_failure_reasons` | Normalized recent failure reason counts |

Prometheus metrics use only bounded labels:

- `proxy_quality_score_bucket{bucket="untested|poor|fair|good|excellent"}`
- `proxy_quality_retention_candidates{decision="below_min_score|hard_failure_evict"}`
- `proxy_quality_failure_reasons_total{reason="<normalized reason>"}`

Failure reasons are normalized before becoming labels. Raw proxy addresses,
ports, URLs, subscription content, and free-form error strings must not appear as
metric labels.

## Cleanup

MCP `cleanup_low_score_proxies` is dry-run by default. It scans stored proxies,
returns eligible cleanup candidates with score explanations, and removes them
only when called with `apply: true`.

The first implementation slice intentionally does not add a background cleanup
job. Automated cleanup should be added only after operators are comfortable with
the dry-run output.
