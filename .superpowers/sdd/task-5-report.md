# Task 5 Report: Surge Parser and Test Fixtures

## Status: Done

## Commits
- `c9eba69` feat(sub): add Surge parser and test fixtures

## Files Changed
- **Modified**: `crates/proxy-sub/src/parser/surge.rs` — replaced stub with full implementation
- **Modified**: `crates/proxy-sub/src/parser/mod.rs` — added 2 integration tests
- **Created**: `crates/proxy-sub/tests/fixtures/surge_sample.txt` — 5-line Surge fixture (socks5, http, ss, vmess, trojan)
- **Created**: `crates/proxy-sub/tests/fixtures/mixed_invalid.txt` — 3-line invalid content fixture

## Test Results
All 87 tests pass (`cargo test -p proxy-sub`), including:
- 15 new Surge-specific tests: detect (5), parse per type (6), malformed (3), Params unit (3), fixture (1)
- 2 new integration tests in `mod.rs`: `test_parse_subscription_surge`, `test_parse_subscription_no_match_fixture`

Clippy clean: `cargo clippy -p proxy-sub -- -D warnings` passes with no warnings.

## Self-Review
- Surge format parsing follows the `splitn(4, ',')` pattern per spec
- `detect()` checks for `=` separator and verifies the first comma-separated field is a known type
- vmess: `ws=true` sets network to "ws"; `ws-path`/`ws-host`/`sni` map correctly; default network is "tcp" when no `ws=true`
- Comments (`#`) and empty lines are skipped in both detect and parse
- Malformed lines (no `=`, too few fields, invalid port) produce `Unknown` variants with warning logs
- `Params` helper parsed comma-separated key=value pairs with case-insensitive matching for boolean checks

## Concerns
- None. Implementation matches the task brief exactly.
