# Implement: Xray active health demotion + route hardening

## Checklist

### 1. Route eligibility (`proxy-core`)

- [ ] Add helper(s) in `route_debug.rs` (or small module) for:
  - fresh success within 900s
  - circuit not open
  - Active encrypted state
- [ ] Update `try_xray` filter + selection (prefer lowest latency among eligible)
- [ ] Unit tests for stale / circuit_open / healthy preference / empty set

### 2. Active revalidation + demotion (`proxy-xray`)

- [ ] Add `active_health_fail_streak` map on `OutboundSync`
- [ ] Implement `revalidate_active_nodes` using existing `validate_candidate` /
      `Validator` targets
- [ ] On success: reset streak; refresh pool proxy quality for that port
- [ ] On fail: increment streak; at threshold 2 call shared demote teardown
- [ ] Invoke revalidate at start of `sync_once` (before or after admission —
      **prefer before admission** so dead slots free first)
- [ ] Extend cycle log / `SyncStats` with demoted count if low-cost
- [ ] Unit tests with mocked/stubbed validation outcomes where possible

### 3. Demotion teardown reuse

- [ ] Factor shared teardown used by stale-remove and demotion (avoid copy-paste
      drift): xray cleanup, port release, store remove, registry mark, cooldown

### 4. Validation commands

```bash
cargo test -p proxy-core
cargo test -p proxy-xray
cargo clippy -p proxy-core -p proxy-xray -- -D warnings
# if workspace coupling requires:
cargo test
cargo clippy -- -D warnings
```

### 5. Post-deploy smoke (after merge/push, not in unit phase)

- HTTP `/api/xray/status` active count vs pool `127.0.0.1` Active entries
- `/api/proxy/check-matrix` on each active local port
- `/api/routes/test?host=openai.com` should not stick to known-dead ports after
  two sync intervals (~60s with default 30s)

## Risky files

| File | Risk |
|---|---|
| `crates/proxy-xray/src/outbound_sync.rs` | lifecycle races on active map / ports |
| `crates/proxy-core/src/route_debug.rs` | overseas routing regressions |
| `crates/proxy-core/src/store.rs` | only if quality refresh API needs touch |

## Rollback points

1. After route filter only (still improves selection without demotion)
2. After demotion path (full behavior)

## Non-goals during implement

- New MCP/API endpoints
- Config UI / YAML schema expansion unless trivial defaults-only fields
- Pending encrypted 243k cleanup
