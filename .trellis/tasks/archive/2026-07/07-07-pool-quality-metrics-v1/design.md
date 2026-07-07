# Design: pool-quality-metrics-v1

## Data Flow

`ProxyStore` remains the source of proxy data and scoring policy. The shared
status collector in `proxy-core::status` will query stored proxies, call
`ProxyStore::explain()` for score/retention semantics, aggregate bounded quality
history, and attach the result to `ServiceStatus`.

```
Redis proxy ZSETs -> ProxyStore::all/explain -> status quality aggregation
  -> ServiceStatus.quality -> REST /api/status
                         \-> MCP service_status
                         \-> render_prometheus_metrics -> /api/metrics
```

## Contracts

Add `QualityStatus` to `proxy-core::status` with aggregate fields only. Keep all
metric labels bounded:

- `bucket`: `untested`, `poor`, `fair`, `good`, `excellent`
- `decision`: `below_min_score`, `hard_failure_evict`
- `reason`: normalized failure categories such as `validation_failed`,
  `timeout`, `bad_status`, `request_failed`, `other`

Do not include protocol-specific proxy identities, hostnames, ports, URLs, or
raw error text in labels.

## Compatibility

Adding `quality` to `ServiceStatus` is additive for JSON consumers. When Redis
quality collection fails, `quality` falls back to defaults while `redis` reports
`error`, matching the existing resilient status behavior.

## Testing

Core unit tests cover empty-pool, bucket classification, stale classification,
failure reason normalization, and Prometheus rendering. API/MCP integration
smoke tests assert the public response shape.
