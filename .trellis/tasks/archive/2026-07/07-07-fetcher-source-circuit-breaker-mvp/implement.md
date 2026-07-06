# Implementation Plan: Fetcher Source Circuit Breaker MVP

## Steps

- [x] Inspect current fetcher status, refresh, API, MCP, and Web Fetchers code paths.
- [x] Add a small circuit state model and deterministic transition helpers.
- [x] Wire automatic refresh to skip open sources and probe after cooldown.
- [x] Wire manual single-source refresh to support explicit probing.
- [x] Extend REST and MCP fetcher status/refresh responses.
- [x] Update Web Fetchers page to show circuit state, failures, error, and next probe.
- [x] Add/adjust unit and integration tests for transitions and public contracts.
- [x] Update roadmap and reusable specs if a lasting convention is learned.
- [x] Run backend tests, clippy, and Web build.

## Validation Commands

```powershell
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
Push-Location web; npm run build; Pop-Location
```

## Validation Results

- `cargo fmt --all --check` passed.
- `cargo test --workspace --all-targets` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `npm run build` passed with the existing Vite chunk-size warning.

## Constraints

- Do not restore `stash@{1}: wip: paused fetcher circuit work` by default.
- Do not include `.codex/config.toml` in commits.
- Do not use direct SSH to the dev address.
