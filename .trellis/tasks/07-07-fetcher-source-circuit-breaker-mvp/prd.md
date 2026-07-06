# Fetcher source circuit breaker MVP

## Background

Fetcher status already records recent refresh results, and the dashboard now exposes fetcher operations. The missing production behavior is source-level backoff: a source that repeatedly fails is still eligible for automatic refresh attempts, which can waste refresh cycles and make pool quality noisy.

This task replaces "keep trying a bad source every time" with a small circuit breaker model that automatically cools failing sources down and lets operators manually probe them.

## Goal

Add source-level circuit breaker states so failing fetchers are cooled down, skipped by automatic refresh while open, and recoverable through half-open probing.

## Non-Goals

- Do not restore or merge any existing stash for fetcher circuit work.
- Do not redesign proxy validation, scoring, or source parsing.
- Do not add persistent database schema unless the current storage path already has a safe, local fit.
- Do not use direct SSH for dev validation.

## Requirements

### F1: Source State

Each configured fetcher exposes a circuit state:

- `closed`: normal operation.
- `open`: automatic refresh skips the source until `next_probe_at`.
- `half_open`: the source is eligible for a probe after cooldown.

The status also includes consecutive failure count, last error, last success, last attempt, and next probe time when available.

### F2: Automatic Backoff

Automatic refresh increments the failure count on failed source refreshes. Once the configured threshold is reached, the source moves to `open` and gets a cooldown deadline.

Automatic refresh skips `open` sources before their deadline and returns a structured skipped result instead of treating the skip as a fresh failure.

### F3: Half-Open Probe

When cooldown expires, the next eligible refresh probes the source as `half_open`.

- Probe success resets the circuit to `closed`.
- Probe failure reopens the circuit and extends cooldown.

### F4: Manual Probe

Per-source manual refresh can explicitly probe an `open` source before the automatic cooldown path would normally use it. The response must make clear whether the call was a probe, a skip, a success, or a failure.

### F5: API, MCP, and Web Surface

`/api/fetchers`, `/api/fetchers/{id}/refresh`, MCP fetcher tools, and the Web Fetchers page expose the circuit state and useful timing/error fields. The UI must not pretend unavailable actions succeeded.

## Acceptance Criteria

- [ ] Unit tests cover `closed -> open`, `open -> skipped`, `open -> half_open`, `half_open -> closed`, and `half_open -> open`.
- [ ] Automatic refresh skips open sources before cooldown expiry without counting the skip as a new failed fetch.
- [ ] Manual refresh can probe an open source and returns a structured result.
- [ ] `/api/fetchers` includes circuit state, failure count, last error, and next probe time.
- [ ] MCP fetcher status includes the same circuit fields as the REST API.
- [ ] Web Fetchers page renders circuit state and next probe/error fields from real API data.
- [ ] `cargo test --workspace --all-targets` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `npm run build` passes for Web changes.

## Notes

- Keep the first implementation in-memory if that matches current fetcher status storage. Persistence can be a follow-up once the state contract proves useful.
- Current direct dev access constraint remains: no direct SSH to the dev address.
