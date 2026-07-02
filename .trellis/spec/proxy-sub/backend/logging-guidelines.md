# Logging Guidelines — proxy-sub

> How logging is done in the subscription discovery and parsing crate.

---

## Overview

`proxy-sub` uses `tracing` for all logging (never the `log` crate). Logs are the primary observability mechanism since the crate runs as a background loop with no user-facing output. Structured fields are used consistently to enable filtering and aggregation.

---

## Log Levels

| Level | When to Use | Examples |
|-------|------------|---------|
| `error` | Truly unexpected states that indicate a bug or infrastructure failure | `reqwest::Client` construction failure |
| `warn` | Expected failures that should be investigated if they persist | Fetch failure, parse error, rate limit, invalid format, Redis deserialization failure |
| `info` | Normal operational milestones | Refresh cycle start/end, discoverer results, format detection, cycle summary |
| `debug` | Detailed operational details useful during development | Cache hit/miss, fetch timeout value, per-URL processing details |

### Decision Rules

- If the system can continue operating correctly despite the condition -> `warn`
- If the condition means a feature is broken and cannot recover -> `error`
- If the condition is a normal step in the workflow -> `info`
- If the condition is only useful when debugging a specific issue -> `debug`

---

## Structured Logging

### Required Fields

All log statements in discoverers and the refresh loop must include the `name` field identifying the component:

```rust
tracing::warn!(name = self.name(), %keyword, "repo search request failed: {e}");
tracing::info!(name = disc.name(), count = urls.len(), "discoverer finished");
```

### URL Context

When logging about a specific subscription URL, include it as a structured field:

```rust
tracing::warn!(url = %url, "fetch failed: {e}");
tracing::debug!(url = %url, "no proxies parsed from subscription");
```

### Summary Fields

The refresh cycle summary uses structured fields for metrics:

```rust
tracing::info!(
    total_basic,
    total_encrypted,
    failed_urls,
    "subscription refresh cycle completed"
);
```

### Format

Use `tracing` macros with structured key-value pairs. Inline the error message using `{e}` (Display) rather than `{:?}` (Debug) for readability:

```rust
// CORRECT
tracing::warn!("V2Ray JSON: parse error: {e}");
tracing::warn!(name = self.name(), %keyword, "repo search request failed: {e}");

// AVOID — Debug format is noisy for error types
tracing::warn!("V2Ray JSON: parse error: {:?}", e);
```

---

## What to Log

### Refresh Loop (`refresh.rs`)

| Event | Level | Fields |
|-------|-------|--------|
| Refresh cycle starting | `info` | — |
| Running discoverer | `info` | `name` |
| Discoverer finished | `info` | `name`, `count` |
| Deduplicated URLs | `info` | `total_urls` |
| Fetch failed | `warn` | `url` |
| No proxies parsed | `debug` | `url` |
| Failed to store basic proxy | `warn` | `url` |
| Failed to store encrypted proxies | `warn` | `url` |
| Refresh cycle completed | `info` | `total_basic`, `total_encrypted`, `failed_urls` |
| Refresh cycle sleeping | `info` | `sleep_secs` |

### Discoverers (`discover/`)

| Event | Level | Fields |
|-------|-------|--------|
| HTTP request failed | `warn` | `name`, `keyword` (if applicable) |
| GitHub rate limited (403/429) | `warn` | `name`, `keyword`, `status` |
| JSON/YAML parse failed | `warn` | `name`, `keyword` (if applicable) |
| Unknown aggregator format | `warn` | `name`, `format` |
| Client construction failure | `error` | — |

### Parsers (`parser/`)

| Event | Level | Fields |
|-------|-------|--------|
| Format detected | `info` | parser `name` (via `parser.name()`) |
| No format detected | `warn` | `len` (content length) |
| Document-level parse failure | `warn` | parser name prefix in message |
| Individual entry parse failure | `warn` | parser name prefix in message |
| Invalid URI component | `warn` | parser name prefix in message |

### Source (`source/`)

| Event | Level | Fields |
|-------|-------|--------|
| Cache hit | `debug` | `url` |
| Fetching (cache miss) | `debug` | `url`, `timeout` |

### Pending Store (`pending.rs`)

| Event | Level | Fields |
|-------|-------|--------|
| Failed to deserialize proxy from Redis | `warn` | — |

---

## What NOT to Log

### Passwords and Secrets

Never log proxy passwords, Shadowsocks passwords, Trojan passwords, or any credential material:

```rust
// FORBIDDEN — logs the password
tracing::debug!("parsed trojan: password={}", password);

// CORRECT — no credential in log
// (just don't log it; the SubscriptionProxy struct carries it internally)
```

### GitHub Tokens

Never log the GitHub API token:

```rust
// FORBIDDEN
tracing::debug!("using GitHub token: {}", token);

// CORRECT — just don't log it
```

### Full Subscription Content

Never log the full subscription content (can be very large and may contain credentials):

```rust
// FORBIDDEN
tracing::debug!("subscription content: {content}");

// CORRECT — log metadata only
tracing::debug!(url = %url, "no proxies parsed from subscription");
```

### Raw Config for Unknown Proxies

The `SubscriptionProxy::Unknown { raw_config }` field may contain credentials. Do not log it at `info` or `debug` level. It is acceptable at `warn` level when reporting a parse failure, but prefer logging just the protocol type:

```rust
// PREFER
tracing::warn!("unsupported protocol: vless");

// ACCEPTABLE (at warn level only, for debugging)
tracing::warn!("unsupported entry: type={}, server={}", proxy_type, server);
```

---

## Log Message Conventions

1. **Prefix with component name** in parser/discoverer messages: `"V2Ray JSON: parse error"`, `"Base64 URI: invalid host:port"`, `"Surge: line has no '=' separator"`
2. **Lowercase after colon**: `"V2Ray JSON: parse error: {e}"` not `"V2Ray JSON: Parse Error: {e}"`
3. **Present tense**: `"fetch failed"` not `"fetch was failed"` or `"fetch has failed"`
4. **Include the failing value** when it helps diagnosis: `"invalid port 'abc'"` not just `"invalid port"`
5. **Use `= %` for Display types** in structured fields: `url = %url` not `url = url.to_string()`
