# Trial Subscription Intake Guide

This guide describes how operators can safely add self-obtained trial or
personal subscription URLs into the proxy pool system.

> **⚠️ Safety boundary:** This system is for adding URLs you already have.
> It must **never** be used to automate account registration, bypass
> CAPTCHAs, harvest trial accounts, or create account factories. The
> codebase contains **zero** auto-registration modules, and this is a
> hard compliance requirement.

---

## End-to-End Flow

```
1. Add URL to config → 2. Preview → 3. Apply → 4. Observe quality → 5. Disable if bad
```

### Step 1: Add subscription URL

Edit `config/settings.yaml`:

```yaml
subscription:
  urls:
    - "https://your-subscription-provider.com/api/v1/client/subscribe?token=YOUR_TOKEN"
```

For multiple URLs, add them as a list. Each URL gets an auto-generated ID
based on its position (e.g., `static-url-1`, `static-url-2`).

> **Do not commit private tokens or subscription URLs to git.** Use
> environment variable substitution or a local override file that is
> `.gitignore`d.

After editing, restart the service for the new URL to be recognized.

### Step 2: Preview (always first)

Preview fetches and parses the source without writing anything to the pool:

**MCP:**
```
subscription_sources()
```
→ Note the source ID (e.g., `static-url-1`)

```
refresh_subscription_source(source="static-url-1", apply=false)
```

**REST:**
```bash
# List sources
curl -s http://localhost:8000/api/subscriptions/sources | jq .

# Preview
curl -s -X POST 'http://localhost:8000/api/subscriptions/sources/static-url-1/refresh' | jq .
```

**Inspect the response:**

| Field | What to check |
|-------|---------------|
| `recommendation.decision` | `apply`, `review`, or `reject` |
| `parsed_count` | Number of nodes parsed from the source |
| `supported_count` | Nodes with recognized protocols (ss, vmess, trojan, vless) |
| `duplicate_count` | Nodes already in the pool |
| `error` | Fetch/parse errors if any |

- `apply` → source meets quality thresholds, safe to apply
- `review` → usable but noisy; apply with caution
- `reject` → source is not safe; `apply=true` is blocked for `reject` sources

### Step 3: Apply

Only after reviewing the preview:

**MCP:**
```
refresh_subscription_source(source="static-url-1", apply=true)
```

**REST:**
```bash
curl -s -X POST 'http://localhost:8000/api/subscriptions/sources/static-url-1/refresh?apply=true' | jq .
```

This writes parsed nodes to the pool and triggers xray activation for
supported encrypted protocols (vmess, vless, trojan, ss with AEAD ciphers).

### Step 4: Observe quality

After applying, check the system status:

**1. Subscription activation report:**
```
subscription_sources()
```
→ Check `last_refresh_report` for parse/activation counts and errors.

**2. Xray activation progress:**
```
xray_status()
```
→ Check `active_nodes`, `failed_nodes`, `activating_nodes`.
→ Target: `active_nodes >= 3` for `pool.tier = stable`.

**3. Pool tier and overall health:**
```
service_status()
```
→ Check `pool.tier`:
  - `stable` = xray active ≥ 3 + WARP healthy ≥ 1 ✓
  - `degraded` = WARP ok but xray < 3
  - `unstable` = no reliable overseas exit

**4. Proxy quality sampling:**
```
explain_proxy_scores(min_score=0.35, overseas=true, alive=true, limit=10)
```
→ Verify overseas proxies meet the D2 quality bar.

**5. Route verification:**
```
route_test(host="openai.com")
```
→ Confirm overseas traffic routes through xray first, then WARP.

### Step 5: Disable if bad

If the source produces poor results:

**1. Remove the URL from config:**
```yaml
subscription:
  urls:
    # - "https://bad-source.example.com/subscribe"  # disabled: low quality
```

**2. Restart the service.**

**3. Clean up low-quality proxies:**
```
cleanup_low_score_proxies(min_score=0.35, apply=false)  # dry-run first
cleanup_low_score_proxies(min_score=0.35, apply=true)   # then apply
```

See `docs/ops-cleanup.md` for the full cleanup playbook.

---

## Batch URL Intake

To add multiple subscription URLs at once:

```yaml
subscription:
  urls:
    - "https://provider-a.com/subscribe?token=TOKEN_A"
    - "https://provider-b.com/subscribe?token=TOKEN_B"
    - "https://provider-c.com/subscribe?token=TOKEN_C"
```

After restart, preview each source individually before applying:

```
refresh_subscription_source(source="static-url-1", apply=false)
refresh_subscription_source(source="static-url-2", apply=false)
refresh_subscription_source(source="static-url-3", apply=false)
```

Apply only the ones with `apply` or acceptable `review` recommendations.

---

## Failure Scenarios

| Scenario | Symptom | Action |
|----------|---------|--------|
| Source fetch fails | `error: "fetch failed"` in preview | Check URL validity; source may be down or blocked |
| All nodes fail xray activation | `xray_status: active_nodes=0, failed_nodes=N` | Check xray config; nodes may use unsupported ciphers or transports |
| Nodes activate but fail validation | `failed_nodes` high, `active_nodes` low | Check `xray.validate_targets` — nodes may not reach overseas targets |
| Pool tier stays `unstable` | WARP 0 healthy + xray 0 active | Check WARP health; subscription may not provide working nodes |
| Source produces only duplicates | `duplicate_count` high, `new_count` low | Source overlaps with existing pool; may still be useful for refresh |

---

## Compliance Checklist

- [ ] No auto-registration code or scripts in the repository
- [ ] No CAPTCHA bypass, email/phone verification automation
- [ ] No batch account creation tools
- [ ] Subscription URLs are operator-provided, not system-generated
- [ ] `reject` recommendations block default apply
- [ ] All intake actions are logged and observable via API/MCP
