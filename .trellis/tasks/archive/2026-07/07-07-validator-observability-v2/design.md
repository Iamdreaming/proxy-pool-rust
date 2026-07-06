# Design: Validator Observability V2

## Scope

This slice extends `proxy_core::validator::ProxyCheckResult`, which is already the shared contract consumed by MCP `check_proxy` and compatibility validation. The business logic stays in `proxy-core`; MCP remains a serializer of core results.

## Data Model

Add:

- `target_url`: validator target URL.
- `target_host`: parsed host from the target URL when available.
- `http_status`: response status code when headers are received.
- `timings`: `ProxyCheckTimings` with request, body-read, and total milliseconds.
- `observed_ip`: exit IP parsed from the response body.
- `observed_country`: location/country code parsed from the response body.

`timings` values are optional by phase because some failures happen before a phase exists.

## Timing Semantics

`request_ms` measures from immediately before `send()` until headers are returned or request send fails. In proxy validation this includes proxy connection, target connection, TLS, and request-to-headers time. It is not labeled as raw TCP connect time.

`body_read_ms` measures response body read time. `total_ms` measures the full check.

## Exit Metadata Parsing

Parse observed exit metadata in one helper owned by `proxy-core`:

- JSON body: `origin` from httpbin-style responses.
- Text body: `ip=` and `loc=` from Cloudflare trace.

Anonymity detection should consume the same parsed observed IP so parsing does not drift.

## Compatibility

`validate_one()` still calls `check_one().await.into_proxy()`. `Proxy` storage still uses rounded `latency_ms` from total elapsed time.

MCP `check_proxy` requires no response shaping change beyond serializing the extended core result.

## Validation

Unit tests should cover:

- serialization of the new success fields;
- request failure timing;
- Cloudflare trace metadata parsing;
- httpbin JSON origin parsing;
- `validate_one` compatibility through `into_proxy()`.
