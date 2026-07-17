# Scenario: Xray Route Eligibility (try_xray / route_test)

## 1. Scope / Trigger

- **Trigger**: Overseas exit prefers xray → WARP → no_proxy. Historical `success_count > 0` made dead Active local SOCKS5 ports still `available=true` in `route_test` and live selection.
- **Owns**: eligibility filter and lowest-latency selection among eligible xray pool entries.
- **Does not own**: active revalidation / demotion lifecycle (see `proxy-xray` active-health-demotion).
- **Code**: `crates/proxy-core/src/route_debug.rs` (`UpstreamSelector::try_xray`, helpers).

## 2. Signatures

```rust
/// Decision D2: 15 minutes.
const XRAY_ROUTE_FRESH_SUCCESS_SECS: i64 = 900;

fn xray_fresh_success_age_secs(proxy: &Proxy, now: DateTime<Utc>) -> Option<i64>;
fn xray_has_validation_evidence(proxy: &Proxy) -> bool;
fn xray_is_route_eligible(proxy: &Proxy) -> bool;

// UpstreamSelector
async fn try_xray(&self) -> Option<u16>; // local SOCKS5 port
```

## 3. Contracts

### Eligibility (`xray_is_route_eligible`)

All must hold:

1. `encrypted_state == Some(Active { .. })`
2. `!circuit::is_circuit_open(proxy)`
3. `xray_has_validation_evidence(proxy)`:
   - `success_count > 0`
   - latest success age ≤ `XRAY_ROUTE_FRESH_SUCCESS_SECS` (900s)

### Freshness source order (`xray_fresh_success_age_secs`)

1. Latest successful `quality_history` sample (`sample.success == true`) by reverse scan → age from `checked_at_unix_secs`.
2. Else if `success_count > 0`: age from `last_check`.
3. Else: `None` (ineligible).

### Selection (`try_xray`)

1. Load SOCKS5 pool entries that are xray local exits (`127.0.0.1` + Active).
2. Filter with `xray_is_route_eligible` **and** gateway runtime cooldown (`xray_failed_until` / connection failure fast path) — same filter for dry-run `route_test` and live selection.
3. Among remaining: prefer **lowest `latency_ms`** (missing latency treated as worst); random among equal-latency ties.
4. Empty set → `None` so WARP / no_proxy fallback runs.

### Complementary layers

| Layer | Role |
|-------|------|
| Active revalidate (proxy-xray) | Tears down dead actives; refreshes quality on success |
| Route eligibility (this spec) | Hides stale / circuit-open / never-successful actives immediately |
| Gateway `XRAY_FAILURE_COOLDOWN` (300s) | Traffic-triggered skip between revalidate cycles |

## 4. Validation & Error Matrix

| Condition | Result |
|-----------|--------|
| Active + fresh success + circuit closed | eligible |
| Active + success older than 900s | ineligible |
| Active + `success_count == 0` | ineligible |
| Active + circuit open | ineligible |
| Non-Active encrypted state / free proxy | not xray exit |
| Store query error in `try_xray` | DEBUG log, return `None` |
| Only stale actives remain | `None` → WARP / no_proxy |

## 5. Good / Base / Bad Cases

- **Good**: Multiple eligible actives → lowest latency port returned; `route_test.available` true for xray.
- **Base**: All actives stale or circuit-open → `try_xray` returns `None`; plan falls through.
- **Bad**: `last_check.is_some() && success_count > 0` without freshness/circuit checks → dead ports stay preferred forever.

## 6. Tests Required

| Case | Assert |
|------|--------|
| No last_check / zero success | `xray_has_validation_evidence` false |
| Fresh success | true |
| Stale beyond 900s | false |
| Circuit open | `xray_is_route_eligible` false even if fresh |
| Preference | lowest latency among eligible; stale siblings ignored |
| Empty eligible set | selection returns `None` |

## 7. Wrong vs Correct

#### Wrong

```rust
fn xray_has_validation_evidence(proxy: &Proxy) -> bool {
    proxy.last_check.is_some() && proxy.success_count > 0
}
// then random among all Active with historical success
```

#### Correct

```rust
fn xray_is_route_eligible(proxy: &Proxy) -> bool {
    matches!(proxy.encrypted_state, Some(EncryptedProxyState::Active { .. }))
        && !circuit::is_circuit_open(proxy)
        && xray_has_validation_evidence(proxy) // success_count>0 && age<=900s
}
// then lowest latency among eligible
```

---

## Design Decision: 15-minute fresh success (D2)

**Context**: Pool `validate_interval_sec` default is 600s; active revalidate also refreshes evidence on success.

**Decision**: 900s (1.5× default validate interval) so scheduler or active revalidate can refresh without immediately starving xray under load; WARP remains fallback when none eligible.

**Constants for MVP**: keep in code; optional YAML later must default to 900.
