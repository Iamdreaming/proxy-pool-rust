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
proxies with score components and retention decisions.

## Cleanup

MCP `cleanup_low_score_proxies` is dry-run by default. It scans stored proxies,
returns eligible cleanup candidates with score explanations, and removes them
only when called with `apply: true`.

The first implementation slice intentionally does not add a background cleanup
job. Automated cleanup should be added only after operators are comfortable with
the dry-run output.
