# Subscription Source Packs

This project can ingest public proxy subscription collections through the existing
subscription pipeline. The recommended public GitHub setup is a hybrid pack:

- curated static or aggregator sources for stable inputs,
- bounded GitHub Search for candidate discovery,
- preview recommendations before writes,
- existing validation, scoring, retention, xray status, and cleanup tools after
  writes.

## Safe Rollout

1. Add one source under `subscription.urls` or `subscription.aggregators`, or
   enable `subscription.github` with a small `max_results`.
2. Preview it first:

   ```bash
   curl "http://localhost:8000/api/subscriptions/sources"
   curl -X POST "http://localhost:8000/api/subscriptions/sources/static-url-1/refresh"
   ```

   MCP equivalent:

   ```json
   {"source":"static-url-1"}
   ```

3. Inspect the returned `recommendation`.
4. Apply only when the decision is `apply` or when a `review` source is known to
   be worth trying:

   ```bash
   curl -X POST "http://localhost:8000/api/subscriptions/sources/static-url-1/refresh?apply=true"
   ```

5. Check post-apply quality with `/api/status`, `/api/proxies/scores`, MCP
   `service_status`, MCP `explain_proxy_scores`, and xray status.
6. If quality is poor, remove/comment the source and use
   `cleanup_low_score_proxies` dry-run before applying cleanup.

## Recommendation Decisions

Preview reports include a source-level `recommendation` object:

| Decision | Meaning |
|---|---|
| `apply` | Source meets the first-version quality thresholds and can be applied. |
| `review` | Source has usable nodes but is noisy, small, or otherwise borderline. |
| `reject` | Source is not safe to apply by default. Normal `apply=true` is blocked. |

The recommendation is based only on pre-apply source metrics. It does not claim
to know latency, anonymity, or real success rate. Those are measured after nodes
enter the pool or xray activation path.

## First-Version Thresholds

The `apply` recommendation requires:

- fetch success rate at least `60%`,
- at least `20` parsed nodes,
- supported protocol ratio at least `50%`,
- unknown node ratio no more than `40%`,
- duplicate node ratio no more than `70%`,
- no dominant fetch/parse error pattern.

Sources are usually `review` when they have usable nodes but miss one of those
thresholds. Sources are `reject` when they have no usable nodes, near-total fetch
failure, extremely low supported protocol ratio, unknown nodes dominating,
extreme duplication, or malformed/private-looking content.

## GitHub Search Candidate Lane

GitHub Search should stay disabled by default:

```yaml
subscription:
  github:
    enabled: false
    max_results: 5
    keywords:
      - "clash free sub"
      - "v2ray free nodes"
      - "free vpn subscriptions clash"
```

Enable it only when you want candidate discovery. Search results are not trusted
curated sources. They still need preview recommendations and post-apply scoring.

## Curated Source Lane

For stable operation, prefer explicit sources:

```yaml
subscription:
  urls:
    - "https://raw.githubusercontent.com/example/repo/main/clash.yaml"
  aggregators:
    - url: "https://raw.githubusercontent.com/example/repo/main/subscriptions.txt"
      format: "text"
```

Do not commit private subscription URLs, account tokens, or secrets. Query
strings and fragments are redacted in operator-facing labels and errors, but
configuration files should still avoid secrets whenever possible.

## Rollback

Rollback is configuration-first:

- remove/comment the source entry,
- set `subscription.github.enabled: false`,
- restart/reload using the normal deployment path,
- run low-score cleanup in dry-run mode before applying cleanup.
