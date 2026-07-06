# Implementation Plan: Validator Observability V2

## Steps

- [x] Read validator, MCP, API/Web consumers, and relevant Trellis specs.
- [x] Extend `ProxyCheckResult` with target metadata, timings, HTTP status, and observed exit metadata.
- [x] Add helper(s) for response metadata parsing and target host parsing.
- [x] Wire `Validator::check_one()` success and failure branches to populate diagnostics.
- [x] Keep `validate_one()` compatibility unchanged.
- [x] Update MCP/spec/docs if the public JSON contract changes.
- [x] Add unit tests for serialization, metadata parsing, and failure timing.
- [x] Run workspace tests and clippy.
- [ ] Commit, push, watch CI, and do no-SSH external checks.

## Validation Commands

```powershell
cargo fmt --all --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

## Validation Results

- `cargo fmt --all --check` passed.
- `cargo test --workspace --all-targets` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.

## Constraints

- Do not restore paused stashes.
- Do not include `.codex/config.toml` in commits.
- Do not use direct SSH to the dev address.
