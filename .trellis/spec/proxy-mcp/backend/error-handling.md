# Error Handling — proxy-mcp

> How errors are handled in the MCP Server crate.

---

## Overview

MCP tools have two return type signatures, each with a distinct error strategy:

| Return Type | Used By | Error Strategy |
|-------------|---------|----------------|
| `Result<String, String>` | `get_proxy`, `get_best_proxy`, `list_proxies`, `remove_proxy` | `Err(format!("Error: {e}"))` for store failures |
| `String` | `check_proxy`, `pool_status`, `warp_status`, `geoip_lookup`, `refresh_pool`, `proxy_stats` | JSON with error/status field, never `Err` |

This split exists because rmcp's `#[tool]` macro supports both signatures. Tools returning `String` **must not fail at the protocol level** — they embed error information in the JSON response body instead.

---

## Error Types

This crate defines **no custom error types**. All errors originate from `proxy-core` and are converted to `String` at the tool boundary.

- `proxy_core::store::ProxyStore` methods return `anyhow::Result<T>`
- `proxy_core::scheduler::SchedulerHandle::refresh()` returns `anyhow::Result<SchedulerResult>`
- These are converted to `Err(format!("Error: {e}"))` or embedded in JSON

---

## Error Handling Patterns

### Pattern 1: Result<String, String> — Store-Backed Tools

For tools that query or mutate the store and can encounter Redis failures:

```rust
#[tool(description = "Get a random working proxy from the pool")]
async fn get_proxy(&self, params: Parameters<ProtocolParam>) -> Result<String, String> {
    let proto = self.resolve_protocol(params.0.protocol.as_deref());
    match self.store.get_random(proto).await {
        Ok(Some(proxy)) => Ok(serde_json::to_string_pretty(&proxy).unwrap_or_default()),
        Ok(None) => Ok("No proxy available for the requested protocol".into()),
        Err(e) => Err(format!("Error: {e}")),
    }
}
```

**Three-branch match**:
1. `Ok(Some(...))` — success with data, serialize to pretty JSON
2. `Ok(None)` — success but empty, return a human-readable message
3. `Err(e)` — store failure, return `Err("Error: {e}")`

**Rule**: Always use `format!("Error: {e}")` as the `Err` variant — the `"Error: "` prefix helps LLM clients distinguish error strings from normal responses.

### Pattern 2: String Return — Graceful Degradation Tools

For tools that should never fail at the MCP protocol level:

```rust
#[tool(description = "Get the current status of the proxy pool")]
async fn pool_status(&self) -> String {
    let http_count = self.store.count(Protocol::Http).await.unwrap_or(0);
    let https_count = self.store.count(Protocol::Https).await.unwrap_or(0);
    let socks5_count = self.store.count(Protocol::Socks5).await.unwrap_or(0);
    serde_json::to_string_pretty(&serde_json::json!({
        "pool": { "http": http_count, "https": https_count, "socks5": socks5_count, "total": ... }
    })).unwrap_or_default()
}
```

**Strategy**: Use `.unwrap_or(0)` / `.unwrap_or_default()` to degrade gracefully. A pool with 0 proxies is a valid state, not an error.

### Pattern 3: Optional Dependency — Feature Not Configured

For tools backed by optional services (GeoIP, WARP):

```rust
#[tool(description = "Get the status of WARP instances")]
async fn warp_status(&self) -> String {
    match &self.balancer {
        Some(balancer) => { /* ... JSON output ... */ }
        None => "WARP not configured".into(),
    }
}
```

**Rule**: Return a plain string message when the backing service is `None`. Do **not** return `Err` — the tool itself works correctly; the feature is simply not enabled.

### Pattern 4: Scheduler Error — Embedded in JSON

```rust
#[tool(description = "Trigger a pool refresh")]
async fn refresh_pool(&self) -> String {
    match self.scheduler_handle.refresh().await {
        Ok(result) => serde_json::to_string_pretty(&serde_json::json!({
            "status": "ok", "fetched": result.fetched, ...
        })).unwrap_or_default(),
        Err(e) => serde_json::to_string_pretty(&serde_json::json!({
            "status": "error", "message": format!("{e}"),
        })).unwrap_or_default(),
    }
}
```

**Rule**: For `refresh_pool`, errors are embedded as `{"status": "error", "message": "..."}` rather than `Err(...)`. This lets the LLM client parse a consistent JSON structure.

---

## Protocol Resolution Fallback

When a user provides an invalid or missing protocol string:

```rust
fn resolve_protocol(&self, protocol: Option<&str>) -> Protocol {
    protocol
        .and_then(Protocol::from_str_loose)
        .unwrap_or(Protocol::Http)
}
```

**Behavior**: Invalid protocol strings silently fall back to `Http`. This is intentional — MCP tool callers (LLMs) may omit the protocol or provide approximate values, and `Http` is the most common default.

**Do not** return an error for invalid protocol strings. The fallback is a feature, not a bug.

---

## JSON Serialization Safety

All tools use `serde_json::to_string_pretty(&...).unwrap_or_default()` for output. The `unwrap_or_default()` ensures:

- Serialization failure produces an empty string rather than a panic
- This is acceptable because all serialized types derive `Serialize` and should never fail in practice

**Forbidden**: Never use `.unwrap()` on `serde_json::to_string_pretty` in tool methods — use `.unwrap_or_default()` instead.

---

## Common Mistakes

| Mistake | Why It's Wrong | Fix |
|---------|---------------|-----|
| Returning `Err` from a `String`-returning tool | Won't compile; `String` tools cannot signal errors at the protocol level | Embed error info in JSON or return a descriptive plain string |
| Using `.unwrap()` on store results in `String` tools | Panics on Redis failure | Use `.unwrap_or(0)` / `.unwrap_or_default()` |
| Forgetting the `"Error: "` prefix in `Err` variants | LLM clients can't distinguish error from normal response | Always use `Err(format!("Error: {e}"))` |
| Returning `Err` when a service is `None` | Feature-not-configured is not an error | Return a plain string like `"WARP not configured"` |
| Adding custom error types | Adds complexity with no benefit — MCP tools only speak `String` | Keep errors as `String`; let `proxy-core` own the error types |
