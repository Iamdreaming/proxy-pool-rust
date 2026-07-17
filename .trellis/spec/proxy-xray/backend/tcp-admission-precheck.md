# Scenario: TCP Precheck Before Xray Admission

## 1. Scope / Trigger

- **Trigger**: Pending encrypted nodes often have dead remote ports. Full admission (local port + xray gRPC config + HTTP Strict validate ~15s) is too expensive to burn on them.
- **Owns**: cheap TCP connect precheck in the admission loop, precheck budget, `SyncStats.precheck_failed`, precheck logging.
- **Does not own**: HTTP validation targets/Strict mode, Active demotion, route eligibility, free-pool non-xray precheck, pending ranking, YAML knobs.
- **Code**: `crates/proxy-xray/src/outbound_sync.rs`, `crates/proxy-xray/src/models.rs` (`SyncStats.precheck_failed`).

## 2. Signatures

```rust
// Constants (MVP; not YAML knobs — Decision D7)
const TCP_PRECHECK_TIMEOUT: Duration = Duration::from_secs(2);       // D2
const TCP_PRECHECK_BUDGET_PER_CYCLE: usize = 200;                    // D5

enum PrecheckError {
    MissingHost,
    InvalidPort,
    Timeout(Duration),
    Connect(#[source] std::io::Error),
}

async fn tcp_precheck_remote(
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<(), PrecheckError>;
// empty/whitespace host or port==0 → Err without dial
// else tokio::time::timeout(timeout, TcpStream::connect((host, port)))

// SyncStats
pub struct SyncStats {
    // ...
    pub precheck_failed: usize, // TCP precheck failures this cycle
}
```

## 3. Contracts

### Admission order (per pending node)

```text
skip if already active / HTTP cooldown / !is_xray_activatable
if precheck_attempts >= 200: break 'labels          // D5
precheck_attempts += 1
TCP connect host:port within 2s                     // D1/D2
on fail:
  stats.precheck_failed += 1
  DEBUG log; continue                               // D3/D4/D6 — no attempt++, no cooldown, no mark_failed
on success:
  if validation_attempts >= attempt_limit: break
  validation_attempts += 1                          // only after precheck pass (D3)
  allocate port → xray config → HTTP validate (unchanged)
```

### Relative to revalidate

- `revalidate_active_nodes` still runs **first** in `sync_once`.
- Precheck does **not** consume revalidate budget or HTTP attempt budget.

### Failure accounting (D3/D4/D6)

| Action on precheck fail | Allowed? |
|-------------------------|----------|
| `stats.precheck_failed += 1` | yes |
| DEBUG log (tag, host, port, elapsed, err) | yes |
| Consume `validation_attempt_limit_per_cycle` | **no** |
| Apply 600s validation cooldown | **no** |
| `status_registry.mark_failed` | **no** |
| Allocate local port / push xray config | **no** |
| Mark Active | **never** (success alone also never marks Active) |

### Observability

- DEBUG fail: `outbound_sync: tcp precheck failed for {tag} {host}:{port} elapsed=... err=...`
- DEBUG success: same shape with `ok`
- INFO cycle summary includes `precheck_failed`

## 4. Validation & Error Matrix

| Condition | Result |
|-----------|--------|
| `host.trim().is_empty()` | `PrecheckError::MissingHost` (no dial) |
| `port == 0` | `PrecheckError::InvalidPort` (no dial) |
| Connect exceeds timeout | `PrecheckError::Timeout(timeout)` |
| Connect I/O error (refused, unreachable, DNS fail) | `PrecheckError::Connect(io)` |
| Handshake completes within timeout | `Ok(())` → continue admission |

## 5. Good / Base / Bad Cases

- **Good**: remote TCP accepts within 2s → precheck ok → HTTP attempt counter increments → existing admission path.
- **Base**: remote port closed/blackholed → precheck fail → `precheck_failed++`, DEBUG, `continue`; node may be retried next cycle without 600s cooldown.
- **Bad**: precheck fail then still allocate port / call `mark_failed` / increment `validation_attempts` — violates D3/D6 and wastes the cycle.

## 6. Tests Required

| Test | Assertion points |
|------|------------------|
| `tcp_precheck_rejects_missing_host_and_zero_port` | empty/whitespace host and port 0 fail without needing network |
| `tcp_precheck_succeeds_against_local_listener` | bind local `TcpListener`, dial same port → Ok |
| `tcp_precheck_fails_on_refused_or_timeout` | closed port / short-timeout blackhole → Err |
| `tcp_precheck_constants_match_decisions` | timeout == 2s, budget == 200 |
| `test_sync_stats_default` | `precheck_failed == 0` |

Commands:

```bash
cargo test -p proxy-xray
cargo clippy -p proxy-xray -- -D warnings
```

## 7. Wrong vs Correct

### Wrong

```rust
// Precheck fail still burns HTTP budget and registry
validation_attempts += 1;
if tcp_precheck_remote(...).await.is_err() {
    self.status_registry.mark_failed(&identity, None, "tcp_precheck_failed").await;
    self.mark_validation_failed(&tag).await; // 600s cooldown
    continue;
}
```

### Correct

```rust
if precheck_attempts >= TCP_PRECHECK_BUDGET_PER_CYCLE {
    break 'labels;
}
precheck_attempts += 1;
if let Err(err) = tcp_precheck_remote(host, port, TCP_PRECHECK_TIMEOUT).await {
    stats.precheck_failed += 1;
    tracing::debug!(/* tag, host, port, elapsed, err */);
    continue; // no attempt++, no cooldown, no mark_failed
}
// only then:
if validation_attempts >= attempt_limit { break 'labels; }
validation_attempts += 1;
// allocate port → xray config → HTTP validate
```

## Design Decisions

| # | Decision | Choice |
|---|----------|--------|
| D1 | Mechanism | TCP connect remote host:port (not ICMP) |
| D2 | Timeout | 2s code constant |
| D3 | Fail consumes HTTP attempt budget? | No |
| D4 | Fail applies 600s cooldown? | No |
| D5 | Max prechecks / cycle | 200 constant |
| D6 | Registry on precheck fail | SyncStats + DEBUG only |
| D7 | Config knobs | Constants only (no new YAML) |

## Residual Risks

- Serial 200×2s worst case ≈ 400s/cycle (most fails are faster; concurrent dials out of scope).
- Precheck success is not liveness proof — HTTP Strict validate remains required for Active.
- Precheck fail is invisible in the status registry by design; operators use cycle stats + DEBUG logs.
