# Ops Cleanup & Pool Tier Guide

## Pool Tier

The service status (`/api/status`, MCP `service_status`) exposes a `pool.tier`
field that reflects overseas exit reliability:

| Tier | Meaning | Condition |
|------|---------|-----------|
| `stable` | Reliable overseas exit | xray active ≥ 3 **and** WARP healthy ≥ 1 |
| `degraded` | Reduced overseas capacity | WARP healthy ≥ 1, xray active < 3 |
| `minimal` | WARP-only overseas | WARP healthy ≥ 1, xray not enabled |
| `unstable` | No reliable overseas exit | WARP 0 healthy, xray 0 active |

The tier is a **read-only signal** derived from existing status fields. It does
not change routing, scoring, or cleanup behavior.

Prometheus metric: `proxy_pool_tier` (0=unstable, 1=minimal, 2=degraded, 3=stable).

---

## Cleanup Playbook

### 1. Dry-run

Always start with a dry-run to inspect candidates before removing anything:

```bash
# MCP tool
cleanup_low_score_proxies(protocol="http", limit=200, min_score=0.35, apply=false)

# REST equivalent
curl -s 'http://localhost:8000/api/proxies/cleanup?protocol=http&limit=200&min_score=0.35' | jq .
```

### 2. Inspect candidates

Review the `candidates` array in the response. Each entry includes:

- `proxy.host`, `proxy.port`, `proxy.protocol` — identity
- `score.score` — current composite score
- `score.retention` — `below_min_score` or `hard_failure_evict`
- `score.trend.recent_samples` — recent validation observations
- `score.trend.recent_success_rate` — success rate in recent window

**Key checks before applying:**

1. **xray active nodes should NOT be in candidates.** If a proxy has
   `source: "xray"` and is still active, it should not be removed by cleanup.
   Verify by cross-referencing `/api/xray/status`.
2. **Stale proxies** (no recent check) are expected candidates. Check
   `trend.recent_samples == 0` or `last_checked_at` older than 1 hour.
3. **Score threshold**: The recommended `min_score=0.35` aligns with the
   overseas profile. Adjust if your pool profile differs.

### 3. Apply

Only after reviewing dry-run output:

```bash
# MCP tool
cleanup_low_score_proxies(protocol="http", limit=200, min_score=0.35, apply=true)

# REST equivalent
curl -s -X POST 'http://localhost:8000/api/proxies/cleanup?protocol=http&limit=200&min_score=0.35&apply=true' | jq .
```

### 4. Frequency

- **Manual / on-demand** is recommended for MVP.
- Automated background cleanup should only be added after operators are
  comfortable with dry-run output (per `docs/score-retention.md`).
- A reasonable cadence is once per day during low-traffic periods.

### 5. Batch strategy

For large pools, process in batches by protocol:

```bash
for proto in http https socks5; do
  cleanup_low_score_proxies(protocol=$proto, limit=200, min_score=0.35, apply=true)
done
```

---

## Noisy Free Fetcher Disable Guide

### Identifying noisy fetchers

Check `/api/fetchers` for fetcher run reports. Key fields:

| Field | Meaning |
|-------|---------|
| `fetched` | Total proxies fetched in last run |
| `stored` | Proxies that passed validation and were stored |
| `validation_survival_rate` | Fraction of parsed proxies that passed validation |
| `consecutive_failures` | Consecutive fetch errors |
| `circuit_state` | `closed` (healthy), `open` (failing), `half_open` (probing) |

### Recommended disable rules

Disable a fetcher when **any** of these conditions persist:

1. `consecutive_failures >= 5` — the source is consistently unreachable.
2. `validation_survival_rate < 0.05` — fewer than 5% of fetched proxies pass
   validation, adding noise without value.
3. `stored == 0` over multiple runs — the source contributes nothing to the pool.

### How to disable

In `config/settings.yaml`, set the fetcher's `enabled` field to `false`:

```yaml
pool:
  fetchers:
    # High-noise sources — disable for overseas profile
    free_proxy_list: { enabled: false }
    clarketm: { enabled: false }
    geonode: { enabled: false }
    # Keep reliable sources
    proxyscrape: { enabled: true }
    thespeedx: { enabled: true }
```

After editing, restart, the disabled fetchers will not run. Their circuit state resets
on restart, so re-enabling is safe.

### Monitoring after disable

1. Check `/api/fetchers` — disabled fetchers should not appear in recent runs.
2. Check `/api/status` — `pool.total` should stabilize or decrease gradually.
3. Check `quality.score_buckets` — the `poor` bucket should shrink over time
   as low-quality proxies expire via normal validation cycles.
