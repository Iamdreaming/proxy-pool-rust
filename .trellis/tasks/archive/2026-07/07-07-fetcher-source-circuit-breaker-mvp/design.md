# Design: Fetcher Source Circuit Breaker MVP

## Approach

Implement the circuit breaker as source-level runtime state owned by the fetcher refresh path. This keeps the first slice small: refresh logic already knows whether a source succeeded, failed, or produced no usable proxies, and API/MCP/Web surfaces already consume fetcher status.

## Circuit Model

The minimal state is:

- `state`: `closed`, `open`, or `half_open`.
- `consecutive_failures`: count of failed source refresh attempts.
- `last_error`: stable message/category for the most recent failure.
- `last_attempt_at`: most recent refresh attempt.
- `last_success_at`: most recent successful refresh.
- `opened_at`: when the source entered open state.
- `next_probe_at`: earliest time automatic refresh may probe again.

Defaults should be conservative and configuration-light. If no existing configuration slot exists, use constants in the fetcher module for the MVP, then make them configurable later if needed.

## State Transitions

- Closed success: reset failure count and error.
- Closed failure: increment failure count; if threshold reached, open with cooldown.
- Open before `next_probe_at`: skip without executing the fetcher and without incrementing failure count.
- Open at or after `next_probe_at`: execute as half-open probe.
- Half-open success: close and reset failure count.
- Half-open failure: open again with an extended cooldown.

## API/MCP Contract

Fetcher status should include the same circuit summary everywhere:

```json
{
  "circuit_state": "open",
  "consecutive_failures": 3,
  "last_error": "request failed: timeout",
  "next_probe_at": "2026-07-07T12:34:56Z"
}
```

Per-source refresh should return whether the source was fetched, skipped, or probed, plus the resulting circuit state.

## Web Contract

The Fetchers page should add a compact circuit state column and expose next probe/error information without fake controls. Manual refresh remains the only operator action in this slice.

## Validation

Primary risk is state-machine correctness, so tests should focus on deterministic transition helpers first, then API/MCP serialization and Web build.
