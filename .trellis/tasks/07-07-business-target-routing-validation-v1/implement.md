# Implementation Plan: business-target-routing-validation-v1

## Checklist

1. [x] Load `trellis-before-dev` and read relevant specs.
2. [x] Add structured validation target config and effective target helper.
3. [x] Extend `Validator` with expected-status handling while preserving default
   `< 400` success.
4. [x] Update scheduler validation to use structured targets.
5. [x] Add built-in business-domain route fallback.
6. [x] Treat GeoIP `UNKNOWN` as overseas only in gateway route planning.
7. [x] Update config example and relevant specs.
8. [x] Add unit tests for routing and validation behavior.
9. [x] Run:
   - [x] `cargo fmt --all --check`
   - [x] `cargo test -p proxy-core route_debug`
   - [x] `cargo test -p proxy-core validator`
   - [x] `cargo test -p proxy-core config`
   - [x] `cargo test -p proxy-core`
   - [x] `cargo clippy -p proxy-core -- -D warnings`
   - [x] `cargo check --workspace`

## Risks

- Built-in business fallback is intentionally opinionated. Explicit non-default
  route rules remain the operator override.
- Expected-status validation proves target reachability, not account-level
  usability.

## Deployment Check

After push and image build, use only GitHub Actions, public HTTP/MCP status, and
MCP/REST route diagnostics. Do not SSH to the dev host.
