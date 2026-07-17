# Scenario: Active Xray Health Revalidation and Demotion

## 1. Scope / Trigger

- **Trigger**: `OutboundSync::sync_once` must keep Active xray nodes routeable only while live validation succeeds. Admission-only validation previously left dead local SOCKS5 ports in the pool.
- **Owns**: post-active revalidation, consecutive-failure demotion, shared teardown, pool quality refresh, registry reason `active_health_check_failed`.
- **Does not own**: free-pool scheduler revalidate, gateway connection cooldown, route eligibility filter (see `proxy-core` xray-route-eligibility).
- **Code**: `crates/proxy-xray/src/outbound_sync.rs`, `crates/proxy-xray/src/models.rs` (`SyncStats.demoted`).

## 2. Signatures

```rust
// Constants (MVP defaults; not YAML knobs)
const ACTIVE_HEALTH_FAIL_THRESHOLD: u32 = 2;          // Decision D1
const ACTIVE_REVALIDATE_BUDGET_CAP: usize = 32;
const ACTIVE_HEALTH_FAIL_REASON: &str = "active_health_check_failed";

// OutboundSync fields
active_health_fail_streak: Arc<RwLock<HashMap<String /*tag*/, u32>>>

// Cycle entry
pub async fn sync_once(&self) -> SyncStats;
// order: revalidate_active_nodes → admission → stale remove

async fn revalidate_active_nodes(&self, stats: &mut SyncStats);
async fn teardown_active_node(&self, tag: &str, kind: TeardownKind) -> bool;

enum TeardownKind {
    StaleRemoved,
    HealthFailed { streak: u32 },
}

fn next_health_fail_streak(current: u32) -> (u32, bool);
// returns (next_streak, should_demote)

fn merge_active_revalidation_quality(
    local_port: u16,
    validated: &Proxy,
    existing_by_port: &HashMap<u16, Proxy>,
) -> Proxy;

// SyncStats
pub struct SyncStats {
    // ...
    pub demoted: usize, // health demotions this cycle
}
```

## 3. Contracts

### Per-cycle revalidate budget

```text
budget = min(active_count, ACTIVE_REVALIDATE_BUDGET_CAP, max(attempt_limit_per_cycle, 1))
```

- Revalidate runs **before** admission so dead slots free first.
- Admission keeps its own `validation_attempt_limit_per_cycle` counter (revalidate does not consume it).
- Snapshot order is `HashMap` iteration (non-deterministic); with ≫32 actives some tags may wait multiple cycles.

### Success path

1. Clear `active_health_fail_streak[tag]`.
2. `merge_active_revalidation_quality` onto the known `127.0.0.1:port` pool entry (preserve `encrypted_config` / `source` / counters).
3. `proxy_store.add(&refreshed)` so route freshness sees recent success evidence.

### Failure path

1. `next_health_fail_streak(current)` → `(next, should_demote)`.
2. If `!should_demote`: log DEBUG, keep Active.
3. If `should_demote`: `teardown_active_node(tag, HealthFailed { streak })` and `stats.demoted += 1` when teardown returns true.

### Teardown sequence (stale + health share)

| Step | Action | Health demotion | Stale remove |
|------|--------|-----------------|--------------|
| 1 | Drop from `active_nodes` | yes | yes |
| 2 | Clear fail streak | yes | yes |
| 3 | Best-effort remove routing/inbound/outbound via gRPC | yes | yes |
| 4 | `port_manager.release(local_port)` | yes | yes |
| 5 | `proxy_store.remove(127.0.0.1:port socks5)` | yes | yes |
| 6 | Registry | `mark_failed(..., ACTIVE_HEALTH_FAIL_REASON)` | `mark_removed` |
| 7 | `mark_validation_failed(tag)` cooldown | **yes** | no |

### In-memory only

- Fail streak is **not** persisted across process restart (accepted: worst case one extra cycle before demotion).

## 4. Validation & Error Matrix

| Condition | Behavior |
|-----------|----------|
| `active_nodes` empty | revalidate returns immediately |
| gRPC disconnected during revalidate | still probes local SOCKS5; teardown gRPC cleanup is best-effort skip when disconnected |
| Pool `all(Socks5)` load fails | revalidate continues with bare `127.0.0.1` probe; **must** merge quality via `merge_active_revalidation_quality` so success path cannot wipe metadata |
| Probe fails once | streak=1, remain Active |
| Probe fails twice consecutively | demote + cooldown + registry Failed |
| Probe succeeds after fail | streak cleared to 0 |
| Tag already removed mid-cycle | skip (no double teardown) |
| `proxy_store.add` / `remove` errors | WARN log; continue (best-effort) |

## 5. Good / Base / Bad Cases

- **Good**: Active node passes revalidate → streak reset, pool `last_check` / success sample refreshed, still routeable.
- **Base**: One timeout → streak=1, still Active; second consecutive timeout → demoted, port released, pool entry gone, registry reason `active_health_check_failed`, tag on failure cooldown.
- **Bad**: Success path writes `validated` bare probe via `proxy_store.add` without merge → wipes `encrypted_config` / `source` when pool load failed.

## 6. Tests Required

| Test | Assertions |
|------|------------|
| `next_health_fail_streak` threshold | 0→(1,false), 1→(2,true); success reset then same path |
| `ACTIVE_HEALTH_FAIL_REASON` stable | equals `"active_health_check_failed"`; threshold == 2 |
| budget clamp | `min(active, 32, attempt_limit)` edges |
| `merge_active_revalidation_quality` | preserves existing metadata; falls back to validated when no existing entry |

Live gRPC/Redis full demote path remains an integration gap (documented residual risk).

## 7. Wrong vs Correct

#### Wrong

```rust
// Admission-only: never revalidate Active; remove only when pending missing.
// On revalidate success: store.add(&validated) with a reconstructed bare probe.
// Stale-remove and demotion copy-pasted teardown (drift).
```

#### Correct

```rust
// sync_once: revalidate_active_nodes first, then admission, then stale remove.
// Fail streak via next_health_fail_streak; demote at 2 consecutive fails.
// Success: merge_active_revalidation_quality then store.add.
// Shared teardown_active_node for StaleRemoved and HealthFailed.
```

---

## Design Decision: Demote after 2 consecutive fails (D1)

**Context**: Live matrix showed ~1/4 Active ports alive while route selection still preferred xray.

**Options**: demote on first fail (noisy) vs 2 consecutive (default) vs N configurable.

**Decision**: fixed threshold 2 (constant). Success resets. Cooldown reuses `validation_failure_cooldown_sec` so demoted tags are not re-admitted every cycle.

**Extensibility**: promote to `XraySettings` only if operators need tuning; defaults must match D1.
