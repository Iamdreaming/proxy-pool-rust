# Proxy Usage Guide

This document describes recommended filter parameters for common proxy
retrieval scenarios via REST API and MCP tools.

## Stable Overseas Proxy

For reliable overseas access (e.g. reaching OpenAI, YouTube, Google):

```json
{
  "min_score": 0.35,
  "max_latency": 2000,
  "overseas": true,
  "alive": true
}
```

### MCP

```
get_best_proxy(min_score=0.35, max_latency=2000, overseas=true, alive=true)
```

### REST

```bash
curl -s 'http://localhost:8000/api/proxies/best?min_score=0.35&max_latency=2000&overseas=true&alive=true' | jq .
```

**Why these values:**

- `min_score=0.35` — aligns with the recommended overseas profile (see
  `docs/score-retention.md`). Proxies below this threshold have poor latency
  or low success rates.
- `max_latency=2000` — 2 seconds is the boundary between "Good" and "Fair"
  latency tiers under the piecewise normalization curve.
- `overseas=true` — restricts to non-CN proxies.
- `alive=true` — excludes circuit-open (recently failed) proxies.

> **Note:** Free-pool proxies that happen to pass these filters are not
> "stable overseas" proxies. The `pool.tier` field in `/api/status` indicates
> whether the pool has reliable overseas exit capacity (`stable` = xray active
> ≥ 3 + WARP healthy ≥ 1).

## Free Pool (No Quality Guarantee)

For casual use where quality is not critical:

```json
{
  "overseas": true
}
```

### MCP

```
get_proxy(overseas=true)
```

### REST

```bash
curl -s 'http://localhost:8000/api/proxies/random?overseas=true' | jq .
```

Free-pool proxies have no minimum score or latency guarantee. They may be
slow, unreliable, or short-lived. Use only for non-critical tasks.

## Domestic Proxy

For CN-internal access:

```json
{
  "overseas": false,
  "alive": true
}
```

### MCP

```
get_best_proxy(overseas=false, alive=true)
```

### REST

```bash
curl -s 'http://localhost:8000/api/proxies/best?overseas=false&alive=true' | jq .
```

## Specific Country

```json
{
  "country": "JP",
  "min_score": 0.35,
  "alive": true
}
```

### MCP

```
get_best_proxy(country="JP", min_score=0.35, alive=true)
```

## High-Anonymity Proxy

```json
{
  "anonymity": "elite",
  "min_score": 0.35,
  "overseas": true,
  "alive": true
}
```

### MCP

```
get_best_proxy(anonymity="elite", min_score=0.35, overseas=true, alive=true)
```

## Filter Reference

| Parameter | Type | Description |
|-----------|------|-------------|
| `protocol` | string | `http`, `https`, `socks4`, `socks5` |
| `country` | string | ISO country code (exact match, e.g. `"US"`) |
| `anonymity` | string | `"elite"`, `"anonymous"`, `"transparent"` |
| `max_latency` | float | Maximum latency in milliseconds |
| `overseas` | bool | `true` = non-CN, `false` = CN only |
| `min_score` | float | Minimum composite score (0.0–1.0) |
| `source` | string | Source name (exact match) |
| `alive` | bool | `true` = exclude circuit-open proxies |
