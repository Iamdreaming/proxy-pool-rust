# Xray active node health demotion and route selection hardening

## Goal

Ensure overseas routes only prefer xray exits that are **currently healthy**.
Dead or stale active xray nodes must be demoted (torn down + removed from the
pool) after sustained health-check failure, instead of remaining routeable after
a one-time admission success.

## Background

Live check on `2026-07-17` against the running service (`git_hash=7cc7d02`):

| Local port | Source | Live matrix check | Pool state |
|---|---|---|---|
| 20000 | `xray:ss:silver.diginetv.online` | alive (DE exit) | Active |
| 20001 | `xray:ss:87.120.108.28` | timeout both targets | Active |
| 20002 | `xray:ss:r3mrcg001287h3p...` | timeout | Active |
| 20003 | `xray:ss:94.156.250.148` | timeout | Active |

Result: **1/4 Active nodes truly alive**, but `route_test` still selected xray
first for `openai.com` / `www.google.com` with `available=true`.

Root causes confirmed in code:

1. **Admission-only validation** — `OutboundSync::validate_candidate` runs once
   at activation; no post-active revalidation/teardown path
   (`crates/proxy-xray/src/outbound_sync.rs`).
2. **Stale removal only** — active nodes are removed only when missing from the
   pending subscription set, not when dead.
3. **Weak route eligibility** — `xray_has_validation_evidence` only requires
   `last_check.is_some() && success_count > 0`
   (`crates/proxy-core/src/route_debug.rs:904-906`), then picks randomly.
4. **Scheduler revalidate does not demote xray** — `revalidate_existing` can
   `mark_failed_with_circuit` but never tears down xray inbound/outbound or
   clears `EncryptedProxyState::Active`.
5. **Gateway cooldown is traffic-triggered only** — dry-run `route_test` never
   dials, so dead nodes still show available.

Related tasks (do **not** duplicate):

- `07-08-vless-xray-validation` — admission validation + VLESS parse.
- `07-07-xray-config-dry-run-and-remove` — operator dry-run / manual remove API.

This task owns **automatic post-active health demotion** and **route selection
hardening**.

## Requirements

### R1: Active-node periodic health revalidation

- Each `OutboundSync` cycle MUST revalidate currently Active nodes through
  their local SOCKS5 ports using the same validation targets as admission
  (`XrayValidationPlan` / pool targets).
- Active revalidation has its own attempt budget so it cannot starve admission
  forever, and admission cannot starve health checks forever (see design for
  split budget defaults).
- Successful revalidation MUST refresh pool proxy quality evidence
  (`last_check`, success sample) for that Active entry.

### R2: Automatic demotion after 2 consecutive failures

**Decision D1:** demote only after **2 consecutive** active revalidation
failures. A success resets the counter to 0.

Demotion MUST:

1. Remove xray inbound / outbound / routing rule (best-effort if gRPC up).
2. Release local SOCKS5 port via `PortManager`.
3. Remove pool `Proxy` entry for `127.0.0.1:<port>`.
4. Drop from in-memory `active_nodes`.
5. Update `XrayStatusRegistry` to a non-active state with reason
   `active_health_check_failed` (or equivalent stable string).
6. Apply existing validation failure cooldown so the tag is not immediately
   re-activated every cycle.

### R3: Route selection prefers currently healthy xray

`UpstreamSelector::try_xray` MUST only consider nodes that are:

1. `EncryptedProxyState::Active`
2. not circuit-open
3. have **fresh success evidence**: a successful check sample / `last_check`
   not older than **15 minutes** (**Decision D2**)
4. `success_count > 0`

Among eligible nodes, prefer lower latency (deterministic-ish) rather than pure
random among all historical successes. If none eligible, return unavailable so
WARP / no_proxy fallback runs.

### R4: Observability

- Log demotion events at INFO with tag, local port, consecutive failures,
  reason.
- `xray/status` active count decreases after demotion; failed/removed reflects
  the transition.
- `route_test` `available` for xray must use the same eligibility filter as
  live selection (no “registered but stale” true).

## Decisions

| # | Decision | Choice | Date |
|---|---|---|---|
| D1 | Demotion threshold | **2 consecutive** active revalidation failures | 2026-07-17 |
| D2 | Route fresh-success window | **15 minutes** | 2026-07-17 |

## Acceptance Criteria

- [ ] AC1: With multiple Active nodes where some fail live validation targets
      twice in a row, those nodes are no longer Active in pool / registry /
      `active_nodes` after the second failed revalidation.
- [ ] AC2: Demotion releases the local port and removes the
      `127.0.0.1:<port>` pool entry (no orphan Active encrypted proxy).
- [ ] AC3: A single flaky failure does **not** demote; the node remains Active
      until a second consecutive failure (or a success resets the counter).
- [ ] AC4: `try_xray` / `route_test` ignores Active nodes whose last success is
      older than 15 minutes or that are circuit-open; with only stale/dead
      xray left, selection falls through to WARP or no_proxy per existing plan.
- [ ] AC5: A still-healthy Active node with fresh success remains selectable
      after siblings are demoted or filtered.
- [ ] AC6: Unit tests cover demotion threshold, teardown side-effects hooks
      (as far as unit-testable), and route eligibility filter.
- [ ] AC7: `cargo test` and `cargo clippy -- -D warnings` pass for touched
      crates.

## Out of Scope

- VLESS parsing / new protocol support (`07-08-vless-xray-validation`).
- Manual dry-run / operator remove API (`07-07-xray-config-dry-run-and-remove`).
- Changing overseas route priority order (xray → WARP → no_proxy stays).
- Pending subscription size explosion investigation.
- MCP `check_proxy(127.0.0.1)` reliability.
- Gateway 9080 tailnet reachability.

## Technical Notes

- Config today: `xray.sync_interval_sec=30`,
  `validation_attempt_limit_per_cycle=50`,
  `validation_failure_cooldown_sec=600`, `max_active_nodes=5000`,
  port range `20000-29999`.
- Inbound listens on `127.0.0.1` only (`proxy-xray` `generate_inbound_json`).
- Admission already uses `Validator::validate_one_against_targets`.
- Gateway `XRAY_FAILURE_COOLDOWN` is 300s on real connection failure; remains
  complementary to demotion.
- Pool `validate_interval_sec` default is 600s; 15m freshness is 1.5× that
  interval so scheduler revalidate can still refresh evidence if active loop
  is busy.
