# Implement: Xray admission TCP precheck

## Checklist

### 1. Models

- [ ] Add `SyncStats.precheck_failed: usize` (default 0)

### 2. Precheck helper (`outbound_sync.rs`)

- [ ] Constants: `TCP_PRECHECK_TIMEOUT` = 2s, `TCP_PRECHECK_BUDGET_PER_CYCLE` = 200
- [ ] `tcp_precheck_remote(host, port, timeout) -> Result<(), ...>`
- [ ] Validate host non-empty / port != 0 before dial
- [ ] `tokio::time::timeout` + `TcpStream::connect`

### 3. Wire into `sync_once` admission loop

- [ ] After activatable + HTTP cooldown checks
- [ ] Enforce precheck budget 200; stop further pending scan when hit
- [ ] On fail: `stats.precheck_failed += 1`, DEBUG, `continue` (no attempt++, no cooldown, no mark_failed)
- [ ] On success: then apply HTTP `validation_attempts` gate and increment
- [ ] Ensure cycle log includes precheck_failed

### 4. Tests

- [ ] Invalid host/port fails
- [ ] Local listener success path (tokio test)
- [ ] Timeout / refused path
- [ ] Budget constant / attempt ordering documented via unit if practical

### 5. Validation commands

```bash
cargo test -p proxy-xray
cargo clippy -p proxy-xray -- -D warnings
```

## Risky files

| File | Risk |
|---|---|
| `crates/proxy-xray/src/outbound_sync.rs` | Admission loop ordering / attempt accounting |
| `crates/proxy-xray/src/models.rs` | SyncStats field consumers |

## Rollback

Delete precheck block; restore `validation_attempts += 1` to pre-port position if needed.

## Non-goals

- YAML knobs
- Concurrent precheck dials
- Registry mark_failed for precheck
