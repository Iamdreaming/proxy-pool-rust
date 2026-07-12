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

### Latency normalization (piecewise-linear)

The piecewise-linear curve keeps proxies distinguishable across the full latency
range. The old `clamp((2000-ms)/2000, 0, 1)` saturated at 2 s, making 2 s and
11 s proxies indistinguishable.

```text
  ms ≤ 1000  → 1.0
 1000 < ms ≤ 2000  → 1.0 − 0.5 × (ms−1000)/1000
 2000 < ms ≤ 5000  → 0.5 − 0.4 × (ms−2000)/3000
 5000 < ms ≤ 10000 → 0.1 − 0.1 × (ms−5000)/5000
 ms > 10000 → 0.0
```

| Latency | Norm | Tier |
|---------|------|------|
| 500 ms | 1.0 | Excellent |
| 1000 ms | 1.0 | Excellent |
| 1500 ms | 0.75 | Good |
| 2000 ms | 0.5 | Good |
| 3000 ms | ≈0.37 | Fair |
| 5000 ms | 0.1 | Fair |
| 7500 ms | 0.05 | Poor |
| 10000 ms | 0.0 | Dead |

Unknown latency is treated as `5000ms`, which maps to `0.1` (was `0.0` under
the old formula).

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

## Target Admission

The `pool.target_admission` field controls how multi-target validation admits
proxies:

| Value | Behavior | Default? |
|-------|----------|----------|
| `quorum` | Any target passing admits the proxy | ✓ |
| `strict` | All targets must pass for admission | |

For overseas profiles, `strict` is recommended to ensure the proxy reaches all
required destinations.

## Recommended Overseas Profile

```yaml
pool:
  min_score: 0.35
  target_admission: strict
  validate_targets:
    - url: "https://cloudflare.com/cdn-cgi/trace"
    - url: "https://api.ipify.org?format=json"
    - url: "https://www.youtube.com"
      expected_statuses: [200, 301, 302]
```

The recommended `min_score` of `0.35` ensures a proxy needs at least ~2 s
latency with decent success rate and anonymity to remain in the pool. Free-list
proxies that fail most targets are naturally excluded.

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
