# Design: Xray active health demotion + route hardening

## Scope / boundaries

| Crate | Owns | Does not own |
|---|---|---|
| `proxy-xray` | Active-set revalidation, consecutive-failure counters, demotion teardown | Free-pool scheduler policy |
| `proxy-core` | `try_xray` eligibility / preference, optional shared helper for freshness | xray gRPC lifecycle |
| `proxy-api` / `proxy-mcp` | No new endpoints required for MVP | Operator manual remove (other task) |

## Architecture

```
OutboundSync::sync_once (each sync_interval)
  1. revalidate_active_nodes()     // NEW: health of current Active set
       - for each active node:
           validate via local SOCKS5 + XrayValidationPlan.targets
           success -> reset fail streak; refresh ProxyStore quality
           fail    -> streak += 1; if streak >= 2 -> demote_node()
  2. existing admission path       // pending -> activate (unchanged contract)
  3. existing pending-stale remove // unchanged
```

Demotion reuses the same teardown sequence already used for stale removal /
activation failure cleanup:

1. `cleanup_xray_config` (routing rule + outbound + inbound)
2. `port_manager.release`
3. `proxy_store.remove(127.0.0.1:port socks5)`
4. remove from `active_nodes`
5. `status_registry.mark_failed(..., "active_health_check_failed")`
6. `mark_validation_failed(tag)` (reuse `validation_failure_cooldown_sec`)

## Data / state

### New in-memory state on `OutboundSync`

```text
active_health_fail_streak: HashMap<String /*tag*/, u32>
```

- Not persisted across process restart (acceptable: next cycles re-probe).
- Cleared on success and on demotion/removal.

### Pool proxy quality refresh on success

On successful active revalidation, write back via existing store success path
(or `store.add` with updated `last_check` / samples) so `try_xray` freshness
sees recent evidence without waiting for free-pool revalidate.

## Route selection (`proxy-core`)

Replace weak evidence check:

```text
// before
last_check.is_some() && success_count > 0

// after
EncryptedProxyState::Active
&& !circuit_open
&& success_count > 0
&& last successful evidence age <= 15 minutes
```

Freshness source of truth (prefer in order):

1. Latest successful `quality_history` sample timestamp, else
2. `last_check` when last sample was success / fail_count did not dominate

If multiple eligible: choose lowest `latency_ms` (fallback: random among ties).

Constants (defaults; optional later config):

| Name | Default | Notes |
|---|---|---|
| `active_health_fail_threshold` | 2 | D1 |
| `xray_route_fresh_success_secs` | 900 | D2 = 15m |
| active revalidate budget / cycle | min(active_count, 32) or share half of attempt limit | avoid starving admission |

Config fields: **prefer constants/defaults in this task** unless wiring is
trivial via existing `XraySettings`. Do not block MVP on new YAML knobs; if
added, they must have serde defaults matching D1/D2.

## Budgeting

Per `sync_once`:

1. Spend up to `active_revalidate_budget` on Active health checks first.
2. Remaining `validation_attempt_limit_per_cycle` (or a parallel admission
   budget equal to current setting) continues to gate new activations.

Rationale: demotion of dead actives is higher value than admitting more dead
candidates when capacity is already partially filled with zombies.

## Compatibility

- No API schema break.
- Overseas exit order unchanged: xray → WARP → no_proxy.
- Existing admission validation path unchanged.
- Gateway runtime `xray_failed_until` cooldown remains as fast path for
  connection failures between revalidate cycles.

## Observability

- INFO: `outbound_sync: demoted {tag} port={p} streak={n} reason=active_health_check_failed`
- INFO cycle summary includes demoted count (extend `SyncStats` if cheap).
- Registry: active↓, failed↑ (or removed↑ if we choose Removed — prefer
  **Failed** with reason for operator clarity).

## Risks / trade-offs

| Risk | Mitigation |
|---|---|
| False demotion from transient timeout | D1: need 2 consecutive fails |
| Revalidate load with many actives | budget cap; sequential or low concurrency |
| Freshness too strict → empty xray set | 15m > default validate_interval 10m; WARP fallback |
| Process restart loses fail streak | acceptable; worst case one extra cycle delay |

## Rollback

- Feature is localized: if regressions, disable active revalidate block behind
  `xray.enabled` already false, or ship a follow-up flag
  `xray.active_health_check_enabled` default true only if needed after review.
- Route filter can be reverted independently of demotion.

## Test plan (design-level)

- Unit: consecutive fail counter demotes at 2, not at 1; success resets.
- Unit: `xray_has_validation_evidence` / new helper rejects stale / circuit_open.
- Unit: `try_xray` returns None when only stale actives exist.
- Existing outbound_sync / route_debug tests updated, no live network required.
