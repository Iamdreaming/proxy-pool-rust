# Task 2 Report: Parser Trait + Clash YAML Parser

**Status:** DONE

## Commits

- `10044a2` feat(sub): add Parser trait and Clash YAML parser

## Test Results

**Command:** `cargo test -p proxy-sub --lib`

```
running 7 tests
test models::tests::test_dedup_key ... ok
test models::tests::test_is_direct_usable ... ok
test parser::clash::tests::test_clash_detect_valid ... ok
test parser::clash::tests::test_clash_detect_invalid ... ok
test parser::clash::tests::test_clash_parse ... ok
test parser::tests::test_parse_subscription_empty ... ok
test parser::tests::test_parse_subscription_no_match ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

**Clippy:** `cargo clippy -p proxy-sub -- -D warnings` — no warnings

## Self-Review

1. **`ws.headers` type mismatch**: The brief used `ws.headers.get("Host")` but `headers` is `Option<HashMap>`, not `HashMap`. Fixed with `ws.headers.as_ref().and_then(|h| h.get("Host").cloned())`.
2. **Clippy `unnecessary_filter_map`**: The brief's `filter_map` always returns `Some(...)`, so Clippy flagged it. Refactored to `.map()` since no entries are ever filtered out.
3. **Dead code warning on `name` field**: Added `#[allow(dead_code)]` since the `name` field is needed for deserialization but not read by the parser logic.

## Concerns

None. All stub parsers (Base64UriParser, V2rayJsonParser, SurgeParser) implement the `Parser` trait with `detect() -> false` so the crate compiles and the detection order is preserved.
