# Quality Guidelines — proxy-sub

> Code quality standards for the subscription discovery and parsing crate.

---

## Overview

`proxy-sub` processes untrusted external data (subscription URLs, GitHub API responses, subscription content in various formats). Code must be defensive, well-tested, and never panic on malformed input.

---

## Forbidden Patterns

### 1. `unwrap()` on external data

Never use `unwrap()` on values derived from external input (parsed JSON, URI components, base64-decoded strings, HTTP responses).

```rust
// FORBIDDEN — panics on malformed subscription data
let port: u16 = parts[2].parse().unwrap();

// CORRECT — graceful fallback
let port: u16 = match parts[2].trim().parse() {
    Ok(p) => p,
    Err(_) => {
        tracing::warn!("invalid port '{}'", parts[2].trim());
        return SubscriptionProxy::Unknown { raw_config: line.to_string() };
    }
};
```

Exception: `reqwest::Client::builder().build()` may use `unwrap_or_else()` with a fallback client, since client construction failure is truly unexpected.

### 2. `expect()` on parsed subscription content

Same as `unwrap()` — subscription content is untrusted.

### 3. Panicking in `Parser::parse` or `Discover::discover`

These trait methods must never panic. They return `Vec<T>` for a reason.

### 4. Modifying the `SubscriptionProxy` enum without updating all parsers

Adding a new variant breaks exhaustive `match` in parsers and `convert.rs`. When adding a variant:
1. Update all `match` arms in parsers
2. Update `protocol_label()`, `host()`, `port()`, `dedup_key()`
3. Update `to_proxy()` / `partition()` in `convert.rs`
4. Add serde roundtrip test in `pending.rs`

### 5. Returning `Result` from `Parser::parse` or `Discover::discover`

These trait methods return `Vec<T>` by design. Use log-and-skip instead (see [error-handling.md](./error-handling.md)).

### 6. Hardcoded credentials or tokens

GitHub tokens must come from configuration (`GitHubSearchConfig.token`), never be embedded in source code.

---

## Required Patterns

### 1. Auto-detection parser registration

Every new parser must be registered in `builtin_parsers()` with correct ordering (most specific first):

```rust
pub fn builtin_parsers() -> Vec<Box<dyn Parser>> {
    vec![
        Box::new(V2rayJsonParser), // JSON: fast reject if not valid JSON
        Box::new(ClashParser),     // YAML: check for `proxies:` key
        Box::new(Base64UriParser), // Base64: decode + check for `://`
        Box::new(SurgeParser),     // Line regex
    ]
}
```

Ordering rationale:
- V2Ray JSON first: JSON parsing is cheap and has clear rejection criteria
- Clash YAML second: YAML is more expensive to parse, but `proxies:` key check is fast
- Base64 URI third: base64 decode is expensive, must come after structured formats
- Surge last: line-by-line regex is the most permissive (highest false-positive risk)

### 2. Fixture-based parser tests

Each parser must have a corresponding test fixture in `tests/fixtures/` and at least one test using `include_str!`:

```rust
const FIXTURE: &str = include_str!("../../tests/fixtures/v2ray_sample.json");

#[test]
fn test_fixture_v2ray_sample() {
    let parser = V2rayJsonParser;
    assert!(parser.detect(FIXTURE));
    let proxies = parser.parse(FIXTURE);
    assert!(!proxies.is_empty());
}
```

### 3. Serde roundtrip tests for `SubscriptionProxy`

Every variant must be tested for JSON serialization roundtrip (used by `PendingStore`):

```rust
#[test]
fn test_subscription_proxy_serialization_roundtrip() {
    let sub = SubscriptionProxy::Shadowsocks { /* ... */ };
    let json = serde_json::to_string(&sub).unwrap();
    let decoded: SubscriptionProxy = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.protocol_label(), "ss");
}
```

### 4. `Unknown` variant for unsupported protocols

Parsers must not silently drop unsupported protocol entries. Map them to `SubscriptionProxy::Unknown` with the raw config preserved:

```rust
_ => SubscriptionProxy::Unknown {
    raw_config: format!("type={}, server={}, port={}", entry.proxy_type, entry.server, entry.port),
},
```

### 5. Default values for optional parser fields

When a subscription format omits optional fields, parsers must provide sensible defaults:
- VMess `network`: default `"tcp"`
- VMess `security`/`cipher`: default `"auto"`
- VMess `alter_id`: default `0`
- Trojan `network`: default `None` (when `"tcp"`)
- Shadowsocks `plugin`/`plugin_opts`: default `None`

---

## Testing Requirements

### Unit Tests (required for every new feature)

| Component | Minimum Tests |
|-----------|--------------|
| `SubscriptionProxy` variant | Serialization roundtrip, `dedup_key`, `is_direct_usable` |
| `Parser::detect` | Positive case, negative case, empty input |
| `Parser::parse` | Each supported protocol variant, unsupported -> `Unknown`, malformed input |
| `Discover` impl | `name()` returns expected value, trait object dispatch |
| `convert::partition` | Mix of Basic + encrypted, all Basic, all encrypted |
| `ContentCache` | Put/get, expiry, eviction |
| Helper functions | Each private helper with edge cases |

### Fixture Tests (required for each parser)

Each parser must have at least one `include_str!` fixture test covering a realistic multi-proxy document.

### Integration Tests (refresh cycle)

`run_refresh_cycle` is tested indirectly through `build_discoverers` (verifying correct discoverer construction from config).

### Test Location

Tests live in the same file as the code under `#[cfg(test)] mod tests { }`. Fixtures live in `crates/proxy-sub/tests/fixtures/`.

---

## Code Review Checklist

- [ ] No `unwrap()` or `expect()` on external data
- [ ] New `SubscriptionProxy` variants update all match arms
- [ ] Parser registered in `builtin_parsers()` if applicable
- [ ] Discoverer registered in `build_discoverers()` if applicable
- [ ] Fixture test added for new parsers
- [ ] Serde roundtrip test for new `SubscriptionProxy` variants
- [ ] Unsupported protocols mapped to `Unknown`, not silently dropped
- [ ] Log messages include context (parser/discoverer name, URL, keyword)
- [ ] No sensitive data (passwords, tokens) in log output
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo test` passes with zero failures
