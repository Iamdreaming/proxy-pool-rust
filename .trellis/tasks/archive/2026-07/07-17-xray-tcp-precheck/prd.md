# Xray admission TCP precheck before HTTP validation

## Goal

Before allocating a local SOCKS5 port and configuring xray for a pending
encrypted node, perform a cheap **TCP connect precheck** to the remote
`host:port`. Nodes that fail the precheck must be skipped without burning
xray gRPC config work or full HTTP validation time, so each sync cycle spends
more of its HTTP attempt budget on candidates that might actually activate.

## Background

Live service (`git_hash=1aa3679`, 2026-07-17):

- Registry ~1 Active / ~187 Failed, almost all `xray validation failed`
- Admission today: `is_xray_activatable` → allocate port → inject xray → HTTP
  via local SOCKS5 (Strict, default Cloudflare trace, 15s timeout)
- No ICMP / no TCP pre-dial; each dead node can waste ~15s after xray config
- Cycle: `validation_attempt_limit_per_cycle=50`, HTTP failure cooldown 600s

Related (do not duplicate):

- `07-17-xray-active-health-demotion` (archived) — post-active revalidation
- `07-08-vless-xray-validation` — admission HTTP validation + VLESS

This task owns **pre-admission TCP reachability filtering only**.

## Requirements

### R1: TCP precheck before expensive admission work

- After `is_xray_activatable` and existing HTTP-validation cooldown check, TCP
  connect to `remote_host:remote_port` with a **2s** timeout **before**:
  - allocating a local port,
  - generating / pushing xray config,
  - running HTTP validation.
- TCP only (not ICMP). Success = handshake completes within timeout.
- Hostnames: async DNS; missing host/port or resolve failure → precheck fail.
- Cap **200** TCP prechecks per `sync_once` (independent of HTTP attempt limit).

### R2: Precheck failure handling

- No port allocation, no xray config.
- Does **not** consume `validation_attempt_limit_per_cycle` (D3).
- Does **not** apply 600s validation cooldown (D4).
- Does **not** call `status_registry.mark_failed` (D6) — only `SyncStats`
  counter + DEBUG log (tag, host, port, elapsed).
- When precheck budget is exhausted, stop scanning further pending candidates
  for precheck in that cycle (HTTP path only runs for candidates that already
  passed precheck earlier in the cycle).

### R3: Precheck success continues existing path

- Success continues: increment HTTP attempt counter → allocate port → xray
  config → HTTP validate → Active or `xray validation failed` + cooldown.
- Precheck success alone must **never** mark Active.

### R4: Observability

- DEBUG log on precheck fail/success with tag, host, port, elapsed.
- `SyncStats.precheck_failed` (and optional `precheck_passed` if useful).
- Cycle summary INFO should include precheck_failed when > 0 if cheap.

## Decisions

| # | Decision | Choice | Date |
|---|---|---|---|
| D1 | Mechanism | TCP connect remote host:port (not ICMP) | 2026-07-17 |
| D2 | Timeout | **2s** code constant | 2026-07-17 |
| D3 | Fail consumes HTTP attempt budget? | **No** | 2026-07-17 |
| D4 | Fail applies 600s cooldown? | **No** | 2026-07-17 |
| D5 | Max prechecks / cycle | **200** constant | 2026-07-17 |
| D6 | Registry on precheck fail | **SyncStats + DEBUG only** (no mark_failed) | 2026-07-17 |
| D7 | Config knobs | **Constants only** for MVP (no new YAML) | 2026-07-17 |

## Acceptance Criteria

- [ ] AC1: Unreachable remote port fails precheck and never gets xray
      inbound/outbound for that attempt.
- [ ] AC2: Precheck success still requires existing HTTP validation before Active.
- [ ] AC3: Behavior matches D2–D6 (2s, no HTTP budget, no cooldown, no registry
      spam, ≤200 prechecks/cycle).
- [ ] AC4: Unit tests for precheck helpers (missing host/port, timeout wiring)
      without live network where possible.
- [ ] AC5: `cargo test -p proxy-xray` and `cargo clippy -p proxy-xray -- -D warnings`
      pass.

## Out of Scope

- HTTP target / Strict vs Quorum changes
- Active demotion / route eligibility
- Pending ranking / orphan Active cleanup
- Free-pool non-xray precheck
- New MCP/API endpoints / YAML knobs

## Technical Notes

- Code: `crates/proxy-xray/src/outbound_sync.rs`, `models.rs` (`SyncStats`)
- Identity: `SubscriptionProxy::host()` / `port()`
- Prefer `tokio::net::TcpStream::connect` + `tokio::time::timeout`
- Do not use blocking DNS on the async runtime
- Move HTTP `validation_attempts += 1` to **after** precheck success so D3 holds
