# Quality Guidelines — proxy-mcp

> Code quality standards for the MCP Server crate.

---

## Overview

`proxy-mcp` is a thin adapter with strict constraints from the rmcp macro system. Quality here means: correct macro usage, consistent error patterns, proper parameter struct design, and adequate test coverage for deserialization.

---

## Forbidden Patterns

### 1. Business Logic in Tool Methods

**Forbidden**: Implementing validation, scoring, filtering, or any domain logic inside a `#[tool]` method.

```rust
// BAD — scoring logic belongs in proxy-core
#[tool(description = "Get best proxy")]
async fn get_best_proxy(&self, params: Parameters<ProtocolParam>) -> Result<String, String> {
    let proxies = self.store.all(proto).await.unwrap_or_default();
    let best = proxies.iter().max_by_key(|p| compute_score(p)); // WRONG
    // ...
}
```

```rust
// GOOD — delegate to proxy-core
#[tool(description = "Get best proxy")]
async fn get_best_proxy(&self, params: Parameters<ProtocolParam>) -> Result<String, String> {
    match self.store.get_best(proto).await {
        Ok(Some(proxy)) => Ok(serde_json::to_string_pretty(&proxy).unwrap_or_default()),
        Ok(None) => Ok("No proxy available".into()),
        Err(e) => Err(format!("Error: {e}")),
    }
}
```

**Why**: `proxy-mcp` is an adapter, not a service layer. Domain logic in tool methods duplicates `proxy-core` and diverges over time.

### 2. `.unwrap()` on Fallible Operations in Tool Methods

**Forbidden**: Using `.unwrap()` on `serde_json::to_string_pretty`, store results, or any fallible operation inside a `#[tool]` method.

```rust
// BAD — panics on serialization failure
Ok(serde_json::to_string_pretty(&proxy).unwrap())
```

```rust
// GOOD — degrades to empty string
Ok(serde_json::to_string_pretty(&proxy).unwrap_or_default())
```

### 3. Mutable State in Tool Methods

**Forbidden**: Taking `&mut self` or modifying `self` fields in `#[tool]` methods.

All tool methods take `&self`. State mutations happen through `Arc`-wrapped shared types (`ProxyStore`, `SchedulerHandle`, `WarpBalancer`, `GeoIPLookup` via `Mutex`).

### 4. Direct Redis Access

**Forbidden**: Importing or using `redis` crate types directly.

All data access goes through `proxy_core::store::ProxyStore`. The MCP crate has no knowledge of Redis.

### 5. `log` Crate Usage

**Forbidden**: Using the `log` crate macros (`info!`, `warn!`, etc.).

Always use `tracing` macros (`tracing::info!`, `tracing::warn!`, etc.).

---

## Required Patterns

### 1. Parameter Struct Derives

Every parameter struct must derive all three:

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MyParam {
    pub required: String,
    pub optional: Option<u32>,
}
```

- `Debug` — for development and error reporting
- `Deserialize` — rmcp deserializes JSON-RPC params into these
- `JsonSchema` — rmcp generates the MCP tool JSON schema from this

### 2. Tool Description Format

Tool descriptions must be imperative sentences in `#[tool(description = "...")]`:

```rust
#[tool(description = "Get a random working proxy from the pool. Optionally specify protocol: http, https, socks4, socks5")]
```

**Rule**: Include the list of valid protocol values in the description for any tool that accepts a `protocol` parameter. This helps LLM clients provide correct values.

### 3. Parameters<T> Wrapper

All tool methods that accept parameters must use `Parameters<T>`:

```rust
async fn my_tool(&self, params: Parameters<MyParam>) -> Result<String, String>
```

Parameterless tools use no wrapper:

```rust
async fn pool_status(&self) -> String
```

### 4. JSON Output via serde_json::json!

For structured output, always use `serde_json::json!` + `to_string_pretty`:

```rust
serde_json::to_string_pretty(&serde_json::json!({
    "key": value,
    "nested": { "a": 1 },
})).unwrap_or_default()
```

**Do not** manually construct JSON strings.

### 5. Protocol Resolution via resolve_protocol

For tools that accept an optional protocol parameter, use the shared `resolve_protocol` helper:

```rust
let proto = self.resolve_protocol(protocol.as_deref());
```

**Do not** call `Protocol::from_str_loose` directly in tool methods — the helper provides consistent fallback behavior.

---

## Testing Requirements

### Minimum Coverage

Every parameter struct must have a deserialization test:

```rust
#[test]
fn test_my_param_deserialize() {
    let json = r#"{"field":"value"}"#;
    let param: MyParam = serde_json::from_str(json).unwrap();
    assert_eq!(param.field, "value");
}
```

### Test Categories

| Category | What to Test | Example |
|----------|-------------|---------|
| Parameter deserialization | Required fields, optional fields, missing fields | `test_protocol_param_deserialize`, `test_protocol_param_optional` |
| Handle clone | `SchedulerHandle` clone works | `test_scheduler_handle_clone` |
| Default values | Fallback behavior for `None` fields | `test_list_proxies_param_deserialize` with missing `limit` |

### What NOT to Test in This Crate

- **Store behavior** — tested in `proxy-core`
- **Validator behavior** — tested in `proxy-core`
- **MCP protocol framing** — tested in `rmcp`
- **Integration tests** (full MCP server startup) — belongs in `proxy-server`

### Lint Requirements

- `cargo clippy -- -D warnings` must pass with zero warnings
- `cargo fmt` must produce no diffs

---

## Code Review Checklist

- [ ] New tool method delegates to `proxy-core`, not implements logic locally
- [ ] Parameter struct has `Debug + Deserialize + JsonSchema`
- [ ] Tool description is an imperative sentence with protocol options listed (if applicable)
- [ ] Error pattern matches the return type (`Result<String, String>` vs `String`)
- [ ] `serde_json::to_string_pretty` uses `.unwrap_or_default()`, not `.unwrap()`
- [ ] Protocol resolution uses `self.resolve_protocol()`
- [ ] Deserialization test added for new parameter struct
- [ ] No `redis`, `log`, or `reqwest` imports
- [ ] No `&mut self` in tool methods
