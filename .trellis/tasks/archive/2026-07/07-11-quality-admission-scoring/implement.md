# Implement: 准入收紧与评分修正

## Ordered checklist

### Slice 1: Latency scoring fix (store.rs)

- [ ] 1.1 Replace `latency_norm` in `score_parts()` with piecewise linear function per design.
- [ ] 1.2 Add helper `fn latency_norm_piecewise(ms: f64) -> f64` with clear comments and the 4-segment table.
- [ ] 1.3 Update unknown-latency default: `unwrap_or(5000.0)` → still 5000 but now maps to 0.1 instead of 0.0.
- [ ] 1.4 Add unit tests in `store.rs` tests module:
  - `latency_norm_piecewise(500.0) == 1.0`
  - `latency_norm_piecewise(1000.0) == 1.0`
  - `latency_norm_piecewise(1500.0) ≈ 0.75`
  - `latency_norm_piecewise(2000.0) == 0.5`
  - `latency_norm_piecewise(3000.0) ≈ 0.367`
  - `latency_norm_piecewise(5000.0) == 0.1`
  - `latency_norm_piecewise(10000.0) == 0.0`
  - `latency_norm_piecewise(11000.0) == 0.0`
  - Score ordering: `score(500ms,elite,5/5) > score(2s,elite,5/5) > score(5s,elite,5/5) > score(10s,elite,5/5)`
- [ ] 1.5 Update existing test assertions that relied on old norm values (search for `0.0` latency contributions in test expectations).
- [ ] 1.6 `cargo test -p proxy-core --lib` passes.

### Slice 2: Target admission config (config.rs + scheduler.rs)

- [ ] 2.1 Add `TargetAdmissionConfig` enum in `config.rs` with `Quorum`/`Strict` variants, `Default = Quorum`.
- [ ] 2.2 Add `#[serde(default)] pub target_admission: TargetAdmissionConfig` to `PoolSettings`.
- [ ] 2.3 Add serde round-trip test for `TargetAdmissionConfig`.
- [ ] 2.4 In `scheduler.rs`, replace hardcoded `TargetAdmission::Quorum` with `self.settings.target_admission.into()`.
- [ ] 2.5 Add `impl From<TargetAdmissionConfig> for TargetAdmission` in `validator.rs` or config.rs.
- [ ] 2.6 `cargo test -p proxy-core --lib` passes.

### Slice 3: Recommended overseas min_score constant + docs

- [ ] 3.1 Add `pub const RECOMMENDED_OVERSEAS_MIN_SCORE: f64 = 0.35;` in `config.rs` or `store.rs`.
- [ ] 3.2 Update `docs/score-retention.md`:
  - Document new piecewise latency curve with table.
  - Document `target_admission` config field.
  - Document recommended overseas min_score 0.35.
  - Document recommended overseas validate_targets (D1).
- [ ] 3.3 Update `config/settings.example.yaml`:
  - Add `target_admission: quorum` with comment.
  - Add commented-out overseas profile example (3 targets + strict + min_score 0.35).
- [ ] 3.4 `cargo test -p proxy-core` and `cargo clippy -p proxy-core -- -D warnings` pass.

### Slice 4: Validation gate

- [ ] 4.1 `cargo fmt --all --check`
- [ ] 4.2 `cargo test --workspace`
- [ ] 4.3 `cargo clippy --workspace -- -D warnings`
- [ ] 4.4 Verify `explain_proxy_scores` returns new latency norms on a live or test proxy.

## Validation commands

```bash
cargo fmt --all --check
cargo test -p proxy-core --lib
cargo test -p proxy-core
cargo clippy -p proxy-core -- -D warnings
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

## Risky files / rollback points

| File | Risk | Rollback |
|------|------|----------|
| `crates/proxy-core/src/store.rs` (score_parts) | Score formula change affects all rankings | Revert commit; old formula is 1 line |
| `crates/proxy-core/src/config.rs` | New enum + field; default preserves old behavior | Remove field; default Quorum |
| `crates/proxy-core/src/scheduler.rs` | Reads new config field | Revert to hardcoded Quorum |
| `docs/score-retention.md` | Doc only | Revert |

## Follow-up checks before task.py start

- [ ] PRD acceptance criteria mapped to slices above.
- [ ] `implement.jsonl` and `check.jsonl` have real entries (not just `_example`).
