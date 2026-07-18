# Design: Scenario-tiered quality routing

## Scope / boundaries

| Crate | Owns | Does not own |
|-------|------|--------------|
| `proxy-core` | Tier enum, default exit tables, routes YAML parse extension, route plan resolution, RouteDecision fields | Xray admission, free-pool scoring formula |
| `proxy-api` / gateway | Surface tier in route_test if they serialize RouteDecision | Own tier policy |

## YAML shape (D4, backward compatible)

### Legacy (still valid)

```yaml
groups:
  direct:
    - "*.cn"
    - default
  free_pool:
    - "github.com"
  warp:
    - "cloudflare.com"
```

### Extended

```yaml
groups:
  direct:
    domains:
      - "*.cn"
      - default
    # no tier → special Direct-only behavior
  free_pool:
    tier: any
    domains:
      - "github.com"
      - "example-low.com"
  openai:
    tier: premium
    domains:
      - "openai.com"
      - "chatgpt.com"
  # optional override:
  # exits: [xray, warp, no_proxy]
```

Parse rules:

1. If `groups.<name>` is a **sequence** → treat as domain list; tier from default map (R3).
2. If **mapping** → read `domains` (required list), optional `tier`, optional `exits`.
3. Unknown `tier` string → error at load (fail fast).
4. `exits` if present must be non-empty subset of known RouteExit names; replaces tier table for that group only (still subject to product tests for premium-without-freepool when tier is premium — if custom exits include free_pool while tier is premium, **prefer**: allow explicit exits as operator override, document footgun; or reject free_pool when tier=premium. **Decision for implement: reject FreePool in exits when tier=premium** to enforce D2).

## Core types

```rust
pub enum QualityTier {
    Any,
    Standard,
    Premium,
}

fn exits_for_tier(tier: QualityTier) -> Vec<RouteExit> { /* D6 tables */ }

fn default_tier_for_group(group: &str) -> Option<QualityTier> {
    // free_pool -> Any, warp/xray -> Premium, custom -> Any
    // direct -> None (Direct-only path)
}
```

`Router` gains:

- `group_tiers: HashMap<String, QualityTier>`
- `group_exit_overrides: HashMap<String, Vec<RouteExit>>` (optional)
- keep `match_route` as today for suffix → group

## Route plan resolution order

Replace ad-hoc `exits_for_known_group` usage with:

```
match host:
  if router present:
    route_match = router.match_route(host)
    if group is direct-like / Direct-only: exits = [Direct]
    else if override exits for group: use override
    else if tier for group: exits = exits_for_tier(tier)
    else: exits = exits_for_tier(Any)  // safe default
  else:
    // existing hardcoded domain helpers may remain as secondary
    // or fold BUSINESS_OVERSEAS into premium when no router
    existing geoip / business_domain paths
```

**MVP preference:** When a `Router` is configured, **tier/group policy wins** over `BUSINESS_OVERSEAS_DOMAINS` for non-default matches. For default matches, keep existing direct_reachable / business_domain / geoip fallbacks unless they conflict — document in implement checklist.

Recommended MVP simplification (lower risk):

1. Router non-default match → tier-based exits only.
2. Router default match → if default group has tier/direct policy, use it; else keep today’s geoip/business helpers.
3. No router → keep today’s behavior unchanged.

## Observability

Extend `RouteDecision`:

```rust
pub tier: Option<String>, // "any" | "standard" | "premium" | null for direct
```

`matched_reason` values may include `route_rule_tier` / keep `route_rule` and set tier field.

## Tests

| Case | Assert |
|------|--------|
| Parse legacy YAML | loads; free_pool→any, warp→premium |
| Parse extended YAML | openai tier premium domains match |
| `exits_for_tier` tables | exact D6 order |
| premium plan never includes FreePool | unit |
| any plan includes FreePool then Warp/Xray | unit |
| RouteDecision includes tier | unit/serialize |
| Reject premium+free_pool override | parse error |

## Risks

| Risk | Mitigation |
|------|------------|
| Breaking routes file in production | dual parse; example + migration note |
| default group = direct makes all unknown hosts direct | document; operators move `default` to free_pool/any group if desired |
| Overlap with BUSINESS_OVERSEAS hardcode | MVP resolution order above |
| Operators expect score tiers | out of scope; follow-up |

## Rollback

Revert router/route_debug changes; legacy YAML keeps working if only dual-parse added carefully. Feature is routing-only.
