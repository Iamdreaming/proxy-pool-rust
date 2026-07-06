# Design: Score Retention Policy

## Scope

First slice: explain current score and provide safe manual cleanup. Do not introduce a new background retention loop yet.

## Core Model

Add score explanation types near `proxy_core::store::score` because the store owns the current formula and weights:

- `ScoreComponent`
  - `raw`: optional numeric/string-ish source value represented through dedicated fields where needed
  - `normalized`: `f64`
  - `weight`: `f64`
  - `contribution`: `f64`
- `ScoreExplanation`
  - `score`
  - `latency`
  - `success`
  - `anonymity`
  - `min_score`
  - `retention`
- `RetentionDecision`
  - `Keep`
  - `BelowMinScore`
  - `HardFailureEvict`

Keep `score(proxy, weights)` by delegating to `explain_score(proxy, weights, min_score).score` or by sharing a private calculation helper.

## Retention Rules

Current behavior:

- Latency normalization: `((2000 - latency_ms) / 2000).clamp(0, 1)`, unknown latency uses `5000`.
- Success rate: if no observations, `0.5`; otherwise `((success_count - fail_count) / total).clamp(0, 1)`.
- Anonymity bonus: elite `1.0`, anonymous `0.5`, transparent/unknown `0.0`.
- Final score: weighted sum.
- Hard eviction: `fail_count > max(8, success_count * 3)`.
- Below-min eviction: score below configured `min_score`.

## API

Add `GET /api/proxies/scores`.

Query parameters:

- `protocol`: optional, defaults to `http`
- `limit`: optional, defaults to `20`
- existing filter fields from `ProxyQuery`

Response:

```json
{
  "protocol": "http",
  "count": 1,
  "proxies": [
    {
      "proxy": {"host": "1.2.3.4", "port": 8080, "protocol": "http"},
      "score": {
        "score": 0.87,
        "min_score": 0.1,
        "retention": "keep"
      }
    }
  ]
}
```

## MCP

Add two tools:

- `explain_proxy_scores`: list stored proxies with score explanations.
- `cleanup_low_score_proxies`: dry-run or remove proxies below min score / hard failure threshold.

`cleanup_low_score_proxies` params:

- `protocol`: optional, default `http`
- `limit`: optional scan cap, default `100`
- `min_score`: optional override, default store configured min score
- `apply`: optional bool, default `false`

## Cleanup Behavior

The cleanup tool scans existing stored proxies for one protocol, computes explanations, selects proxies whose retention decision is not `keep`, and removes them only when `apply == true`.

Dry-run and apply return the same candidate shape; apply adds `removed`.

## Compatibility

- Existing score order stays unchanged.
- Existing API/MCP proxy list outputs stay unchanged.
- No config migration.

## Rollback

Revert API/MCP additions and score explanation types. Stored Redis data remains compatible because proxy payloads do not change.
