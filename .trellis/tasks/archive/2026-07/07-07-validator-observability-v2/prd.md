# Validator observability v2

## Background

`check_proxy` already returns structured success/failure and stable error categories, but operators still cannot see enough detail to answer: which target was checked, how long the request phase took, whether the response body exposed an exit IP, or which country the target observed.

This task implements the first safe slice of validator observability without rewriting the validator transport layer.

## Goal

Add richer single-target proxy validation diagnostics for target identity, phase timings, HTTP status, and observed exit metadata.

## Non-Goals

- Do not implement multi-target validation in this slice.
- Do not claim precise TCP connect timing from reqwest internals; expose request-to-headers, body-read, and total elapsed timings.
- Do not change the `validate_one() -> Option<Proxy>` compatibility contract used by batch validation.
- Do not use direct SSH for dev validation.

## Requirements

### F1: Target Metadata

`ProxyCheckResult` includes the target URL and parsed target host for every result.

### F2: Phase Timings

Successful and failed checks include a `timings` object where relevant:

- `request_ms`: elapsed time from request start until headers or request failure.
- `body_read_ms`: elapsed time spent reading the response body.
- `total_ms`: total elapsed time for the check.

### F3: HTTP Response Metadata

When a response is received, include `http_status`. Bad HTTP status failures must also include status and timings.

### F4: Observed Exit Metadata

When the response body exposes an origin IP, return `observed_ip`. When the body exposes a country/location code, return `observed_country`.

The first implementation must support:

- Cloudflare trace: `ip=` and `loc=`.
- httpbin-style JSON: `origin`.

### F5: MCP Compatibility

MCP `check_proxy` keeps returning JSON through the same tool, but the JSON includes the new fields from `ProxyCheckResult`.

## Acceptance Criteria

- [ ] `ProxyCheckResult` serializes `target_url`, `target_host`, `timings`, `http_status`, `observed_ip`, and `observed_country` when present.
- [ ] Success results include total/request/body timings and observed IP metadata when parseable.
- [ ] Bad-status and request-failure results include useful timing data.
- [ ] `validate_one()` still returns `Some(Proxy)` only for alive proxies.
- [ ] MCP `check_proxy` uses `Validator::check_one()` without duplicating parsing logic.
- [ ] `cargo test --workspace --all-targets` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.

## Notes

- Multi-target checks and precise TCP/TLS breakdown can be planned after this contract is stable.
