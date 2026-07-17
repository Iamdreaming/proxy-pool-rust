# Design: Xray admission TCP precheck

## Scope / boundaries

| Crate | Owns | Does not own |
|---|---|---|
| `proxy-xray` | TCP precheck in admission loop, SyncStats counters, constants | Free-pool validator, pending ranking |
| `proxy-core` | No change unless a shared dial helper is desired (prefer keep local) | HTTP validation semantics |

## Admission flow (after)

```
for pending node (ss/vmess/trojan/vless):
  skip if already active / HTTP cooldown / !activatable
  if precheck_attempts >= 200: break scanning  // D5
  precheck_attempts += 1
  TCP connect host:port within 2s              // D1/D2
  on fail:
    stats.precheck_failed += 1
    DEBUG log; continue                       // D3/D4/D6
  on success:
    if validation_attempts >= attempt_limit: break
    validation_attempts += 1                  // only after precheck pass
    allocate port → xray config → HTTP validate (unchanged)
```

Order relative to active revalidate: unchanged
(`revalidate_active_nodes` still first).

## Constants (D7)

```rust
const TCP_PRECHECK_TIMEOUT: Duration = Duration::from_secs(2);
const TCP_PRECHECK_BUDGET_PER_CYCLE: usize = 200;
```

No new `XraySettings` fields in MVP.

## Dial helper

```rust
async fn tcp_precheck_remote(host: &str, port: u16, timeout: Duration) -> Result<(), PrecheckError>;
// uses tokio::net::TcpStream::connect((host, port)) under tokio::time::timeout
// missing/empty host or port==0 → Err immediately
```

Keep unit-testable pure helpers for input validation; dial path tested with a
local `TcpListener` bind when feasible, or timeout-only unit coverage.

## SyncStats

```rust
pub struct SyncStats {
    // existing...
    pub precheck_failed: usize,
    // optional: pub precheck_ok: usize,
}
```

## Observability

- DEBUG: `outbound_sync: tcp precheck failed for {tag} {host}:{port} elapsed=... err=...`
- DEBUG: success at trace/debug only (avoid noise)
- INFO cycle line: include `precheck_failed` when logging stats
- **No** `mark_failed(..., "tcp_precheck_failed")` per D6

## Risks

| Risk | Mitigation |
|---|---|
| Hostnames resolve slowly | Connect timeout covers total wait; 2s cap |
| False negative (slow but live TCP) | 2s chosen; can raise later via constant/config |
| Serial 200×2s worst case = 400s cycle | Most failures faster than timeout; optional later concurrency out of scope |
| Skipping registry makes failures less visible | SyncStats + logs; HTTP failures still registry |

## Rollback

Feature is localized to admission pre-filter; remove precheck block to restore
prior behavior. Active demotion / route filter unaffected.

## Test plan

- Unit: invalid host/port → fail without dial
- Unit: connect to bound local listener within timeout → ok
- Unit: connect to closed port / blackhole with short timeout → err
- Integration-style not required for MVP
