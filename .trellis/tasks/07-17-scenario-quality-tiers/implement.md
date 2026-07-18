# Implement: Scenario-tiered quality routing

## Checklist

### 1. Types & tables (`proxy-core`)

- [x] Add `QualityTier` (+ serde/display as `any`/`standard`/`premium`)
- [x] `exits_for_tier(tier) -> Vec<RouteExit>` per D6
- [x] `default_tier_for_group(name) -> Option<QualityTier>` per R3
- [x] Unit tests for tables

### 2. Router YAML (`router.rs`)

- [x] Dual-parse `groups` values: sequence vs mapping
- [x] Store per-group tier + optional exit overrides
- [x] Direct group / missing tier handling
- [x] Reject invalid tier; reject premium overrides that include FreePool
- [x] Tests: legacy + extended YAML fixtures

### 3. Route selection (`route_debug.rs`)

- [x] Resolve exits from group tier/override when Router matches
- [x] Preserve Direct-only for direct group
- [x] Preserve no-router / geoip paths per design MVP order
- [x] AC2/AC3 unit tests with mock store/balancer as existing tests do

### 4. Diagnostics

- [x] Add `tier` to `RouteDecision` (and any API DTO if duplicated)
- [x] Ensure `route_test` / MCP path returns it if it reuses RouteDecision

### 5. Example config

- [x] Update `config/routes.example.yaml` with tier examples + comments
- [x] Document default group semantics

### 6. Validation commands

```bash
cargo test -p proxy-core   # 173 passed
cargo clippy -p proxy-core -- -D warnings
# if API DTOs touched:
cargo test -p proxy-api
cargo clippy -p proxy-api -- -D warnings
```

## Risky files

| File | Risk |
|------|------|
| `crates/proxy-core/src/router.rs` | YAML compat |
| `crates/proxy-core/src/route_debug.rs` | Exit order regressions for overseas/geoip |
| `config/routes.example.yaml` | Operator docs |

## Rollback

Revert the three areas above; no storage migration.

## Non-goals

- Xray admission changes
- min_score filters
- New MCP tools
