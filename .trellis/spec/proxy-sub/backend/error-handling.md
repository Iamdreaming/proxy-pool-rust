# Error Handling — proxy-sub

> How errors are handled in parsers, discoverers, and the refresh pipeline.

---

## Overview

`proxy-sub` follows a **log-and-skip** error handling philosophy. Subscription data is inherently unreliable — URLs go offline, formats are malformed, and GitHub rate-limits requests. The crate must never crash or abort a refresh cycle due to a single source or entry failure. Errors are logged at the appropriate level and the problematic item is skipped.

---

## Error Types

### No Custom Error Enum

`proxy-sub` does not define a custom error type. It uses:

- **`anyhow::Result`** for fallible operations that propagate to callers (`PendingStore`, `SubscriptionSource::fetch`)
- **`Option<T>`** for parse failures within individual entries (silently skipped via `filter_map`)
- **`Vec<SubscriptionProxy>`** return from `Parser::parse` — empty vec signals total parse failure
- **`Vec<String>`** return from `Discover::discover` — empty vec signals discovery failure

The only local error type is `base64_uri::DecodeError`, a private enum used internally for base64 decode failures. It is never exposed publicly.

---

## Error Handling Patterns

### Pattern 1: Log-and-Skip in Parsers

Individual malformed entries are logged at `warn` and skipped. The parser continues processing remaining entries.

```rust
// In V2rayJsonParser::parse — JSON parse failure for the entire document
let root: Value = match serde_json::from_str(trimmed) {
    Ok(v) => v,
    Err(e) => {
        tracing::warn!("V2Ray JSON: parse error: {e}");
        return Vec::new();
    }
};

// In parse_outbound — individual entry failure returns None (filtered by filter_map)
fn parse_outbound(ob: &Value) -> Option<SubscriptionProxy> {
    let protocol = ob.get("protocol").and_then(|v| v.as_str())?;
    // ... missing fields naturally produce None
}
```

```rust
// In Base64UriParser — malformed URI logged and mapped to Unknown
fn parse_basic(rest: &str, protocol: Protocol) -> SubscriptionProxy {
    let (host, port) = match split_host_port(rest) {
        Some(pair) => pair,
        None => {
            tracing::warn!("Base64 URI: invalid basic URI: {rest}");
            return SubscriptionProxy::Unknown {
                raw_config: format!("{}://{rest}", protocol.scheme()),
            };
        }
    };
    // ...
}
```

**Key rule**: Parsers never return `Err`. They return `Vec<SubscriptionProxy>` — empty on total failure, partial on some failures. Malformed entries become `SubscriptionProxy::Unknown` (preserving raw config for debugging) or are silently dropped.

### Pattern 2: Log-and-Return-Empty in Discoverers

Network errors, rate limits, and parse failures in discoverers are logged and an empty URL list is returned. The refresh loop continues with URLs from other discoverers.

```rust
// In GitHubSearchDiscover::search_repos
let resp = match self.request(&url).send().await {
    Ok(r) => r,
    Err(e) => {
        tracing::warn!(name = self.name(), %keyword, "repo search request failed: {e}");
        return Vec::new();
    }
};

// Rate limit handling
if status.as_u16() == 403 || status.as_u16() == 429 {
    tracing::warn!(name = self.name(), %keyword, %status, "GitHub rate limited");
    return Vec::new();
}
```

```rust
// In AggregatorDiscover::discover — unknown format
match self.config.format.as_str() {
    "text" => parse_text_list(&text),
    "json" => parse_json_list(&text),
    "yaml" => parse_yaml_list(&text),
    other => {
        tracing::warn!(name = self.name(), format = other, "unknown format");
        Vec::new()
    }
}
```

**Key rule**: `Discover::discover` returns `Vec<String>`, never `Result`. All errors are internal — the caller only sees successfully discovered URLs.

### Pattern 3: Log-and-Continue in Refresh Loop

The refresh loop (`run_refresh_cycle`) handles per-URL failures gracefully and continues processing remaining URLs.

```rust
for url in &all_urls {
    let content = match source.fetch(url).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(url = %url, "fetch failed: {e}");
            failed_urls += 1;
            continue;
        }
    };

    // Store failures are logged but don't stop the loop
    for proxy in &basics {
        if let Err(e) = store.add(proxy).await {
            tracing::warn!(url = %url, "failed to store basic proxy: {e}");
        }
    }
}
```

### Pattern 4: Result Propagation for Infrastructure Operations

`PendingStore` and `SubscriptionSource::fetch` return `anyhow::Result` because their callers (the refresh loop) need to decide how to handle Redis/HTTP failures.

```rust
// PendingStore — Redis operations return Result
pub async fn store_batch(&self, nodes: &[SubscriptionProxy]) -> Result<()> { ... }
pub async fn get_pending(&self, protocol_label: &str, limit: usize) -> Result<Vec<SubscriptionProxy>> { ... }
```

Exception within `get_pending`: individual deserialization failures from Redis are logged and skipped rather than propagated:

```rust
for m in members {
    match serde_json::from_str::<SubscriptionProxy>(&m) {
        Ok(p) => result.push(p),
        Err(e) => tracing::warn!("failed to parse pending proxy from redis: {e}"),
    }
}
```

### Pattern 5: Graceful Degradation for Client Construction

`reqwest::Client` construction failures fall back to a default client rather than panicking:

```rust
let client = reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(config.timeout_sec))
    .build()
    .unwrap_or_else(|e| {
        tracing::error!("failed to build reqwest client for GitHub search: {e}");
        reqwest::Client::new()
    });
```

---

## API Error Responses

Not applicable — `proxy-sub` has no HTTP API surface. It is consumed internally by `proxy-server`.

---

## Common Mistakes

| Mistake | Why It's Wrong | Correct Approach |
|---------|---------------|-----------------|
| Returning `Result` from `Parser::parse` or `Discover::discover` | Breaks the log-and-skip contract; callers would need to handle per-entry errors | Return `Vec<T>` — empty on total failure, partial on some failures |
| Using `unwrap()` on parsed fields in parser code | Panics on malformed subscription data (which is common) | Use `?` on `Option` within `fn -> Option<T>` helpers, or provide defaults |
| Propagating a single URL's fetch error to abort the entire refresh cycle | One bad URL should not prevent processing of hundreds of others | Log the error, increment `failed_urls`, `continue` |
| Logging at `error` level for expected failures (rate limits, offline URLs) | These are normal operational conditions, not bugs | Use `warn` for expected failures; reserve `error` for truly unexpected states (e.g., client construction failure) |
| Swallowing errors silently (no log) | Makes debugging impossible when subscriptions stop working | Always log at `warn` or higher before skipping |
| Forgetting to map unparseable entries to `Unknown` | Data is lost silently; no way to investigate why a node wasn't added | Map to `SubscriptionProxy::Unknown { raw_config }` to preserve the original data |
