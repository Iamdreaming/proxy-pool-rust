# Scenario: Quality Tiers (route exit order)

## 1. Scope / Trigger

- **Trigger**: Different hosts need different proxy quality. Some only need "any proxy"; high-value sites must never use dirty free-pool IPs.
- **Owns**: `QualityTier`, routes YAML dual-parse, exit-order tables, `RouteDecision.tier` diagnostics.
- **Does not own**: xray admission/validation, free-pool scoring, source-quality tiers, capability tags (ChatGPT prefer).
- **Code**:
  - `crates/proxy-core/src/router.rs` — `QualityTier`, `Router` tier/override storage, YAML parse
  - `crates/proxy-core/src/route_debug.rs` — `exits_for_tier`, `resolve_group_policy`, plan resolution
  - `config/routes.example.yaml` — operator contract
  - `web/src/types/index.ts` — `RouteDecision.tier?`

## 2. Signatures

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityTier { Any, Standard, Premium }

impl QualityTier {
    pub fn as_str(self) -> &'static str; // "any" | "standard" | "premium"
    pub fn parse(s: &str) -> Result<Self, String>;
}

pub fn default_tier_for_group(group: &str) -> Option<QualityTier>;
// direct → None; free_pool → Any; warp|xray → Premium; other → Any

impl Router {
    pub fn from_yaml_str(text: &str) -> Result<Self, String>;
    pub fn tier_for(&self, group: &str) -> Option<QualityTier>;
    pub fn exit_override_for(&self, group: &str) -> Option<&[String]>;
    pub fn is_direct_only(&self, group: &str) -> bool;
}

pub fn exits_for_tier(tier: QualityTier) -> Vec<RouteExit>;

// Diagnostics
pub struct RouteDecision {
    // ...
    pub tier: Option<String>, // snake_case label when a tier applies
}
```

## 3. Contracts

### Exit order tables (D6)

| Tier | Order |
|------|-------|
| `any` | FreePool → Warp → Xray → NoProxy |
| `standard` | Xray → Warp → FreePool → NoProxy |
| `premium` | Xray → Warp → NoProxy (**never** FreePool) |
| direct-only | Direct only (not a tier) |

### Default tier map (R3) when YAML omits `tier`

| Group name | Tier |
|------------|------|
| `direct` | none (Direct-only) |
| `free_pool` | `any` |
| `warp` / `xray` | `premium` |
| other custom | `any` (safe default) |

### Routes YAML dual-parse

**Legacy** (still valid):

```yaml
groups:
  free_pool:
    - "github.com"
```

**Extended**:

```yaml
groups:
  openai:
    tier: premium
    domains:
      - "openai.com"
    # exits: [xray, warp, no_proxy]  # optional override
    # scene: latency                 # optional hint
```

- `exits` names: `direct` | `free_pool` | `warp` | `xray` | `no_proxy`
- Unknown tier / unknown exit name → load error
- **D2 hard boundary**: `tier: premium` + `exits` containing `free_pool` → load error

### Plan resolution order (MVP, Router present)

1. Non-default route match → group tier / exits override (domain helpers skipped)
2. Default match → `direct_reachable` / `business_domain` helpers, then default-group policy
3. No router → domain helpers → geoip (`overseas` uses premium table; domestic Direct)

### Asymmetric fail policy (D2/D3)

- **premium** never selects FreePool (table + override reject)
- **any** may fall through to Warp/Xray when FreePool empty/failing
- Hardcoded `BUSINESS_OVERSEAS_DOMAINS` uses premium-like exits when no non-default route match

### Behavior change note

Legacy group name `warp` previously implied Warp-first order. Default tier is now **premium** → Xray → Warp → NoProxy. Operators needing Warp-first must set:

```yaml
warp:
  tier: premium   # or omit; default is premium
  domains: [...]
  exits: [warp, xray, no_proxy]
```

## 4. Validation & Error Matrix

| Condition | Result |
|-----------|--------|
| Legacy domain list | loads; tiers from R3 |
| Extended + valid tier | loads; `tier_for` set |
| `tier: gold` | `Err` containing `unknown quality tier` |
| premium + `exits: [..., free_pool, ...]` | `Err` containing premium/free_pool hard boundary |
| `exits: [vpn]` | `Err` containing `unknown exit` |
| empty `exits: []` | `Err` non-empty required |
| group without tier & without exits (`direct`) | Direct-only plan |
| missing `default` entry | `Err` routes must declare default |

## 5. Good / Base / Bad Cases

- **Good**: `openai` tier=premium → plan exits exclude FreePool; `RouteDecision.tier == "premium"`.
- **Base**: legacy `free_pool` list → tier=any; FreePool first, may borrow Warp/Xray.
- **Bad**: treating tier as min_score filter or changing xray admission based on tier (out of scope).

## 6. Tests Required

| Case | Assert |
|------|--------|
| `exits_for_tier` tables | exact D6 order; premium has no FreePool |
| R3 default map | direct/free_pool/warp/xray/custom |
| legacy YAML load | tiers + match |
| extended YAML tier + override | stored correctly |
| reject unknown tier / premium+free_pool / unknown exit | error strings |
| any vs premium plan order | different first exit |
| override standard exits | plan uses override list |
| `routes.example.yaml` | parses; free_pool=any, warp=premium |
| `RouteDecision.tier` | present on tiered plans |

## 7. Wrong vs Correct

#### Wrong

```yaml
# premium cannot borrow free_pool
openai:
  tier: premium
  domains: ["openai.com"]
  exits: [xray, free_pool, no_proxy]
```

```rust
// Do not fold FreePool into premium table "for availability"
QualityTier::Premium => vec![RouteExit::Xray, RouteExit::Warp, RouteExit::FreePool, RouteExit::NoProxy]
```

#### Correct

```yaml
openai:
  tier: premium
  domains: ["openai.com"]
  # exits optional; default table has no free_pool
```

```rust
QualityTier::Premium => vec![RouteExit::Xray, RouteExit::Warp, RouteExit::NoProxy]
```

## Design Decisions

| ID | Choice |
|----|--------|
| D1 | Tier = exit-type order tables (not score bands) |
| D2 | premium hard boundary vs FreePool |
| D3 | any may borrow higher exits |
| D4 | dual-parse routes YAML |
| D5 | routing-layer only (no admission change) |
| D6 | tables above |

## Common Mistakes

1. **Assuming unmatched hosts go overseas**: whichever group holds `default` wins; example puts `default` under `direct`.
2. **Expecting warp group to prefer WARP first**: default is premium order (Xray first); use `exits` override.
3. **Changing xray admission for tiers**: out of scope — only gateway exit order changes.
