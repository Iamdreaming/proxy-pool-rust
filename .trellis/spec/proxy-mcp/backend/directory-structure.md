# Directory Structure — proxy-mcp

> How the MCP Server crate is organized.

---

## Overview

`proxy-mcp` is a single-file library crate. All tool definitions, parameter structs, and the `ServerHandler` implementation live in `src/lib.rs`. There are no sub-modules because the crate's responsibility is narrow: adapt `proxy-core` and `proxy-sub` services to the MCP protocol.

---

## Directory Layout

```
crates/proxy-mcp/
├── Cargo.toml          # Dependencies: proxy-core, rmcp, schemars, serde, serde_json, tokio, tracing, anyhow
└── src/
    └── lib.rs          # ProxyPoolMcp struct, parameter structs, all tool impls, ServerHandler, tests
```

---

## Module Organization

### Single-File Design

The entire crate is one `lib.rs` because:

1. **Narrow scope** — only MCP tool definitions and parameter structs; no business logic
2. **Macro-driven** — `#[tool_router]` and `#[tool_handler]` must be applied to the same struct, making file splitting awkward
3. **Small surface** — 10 tools, 5 parameter structs, 1 struct, 1 trait impl

### Internal Layout Order (within `lib.rs`)

```
1. Crate-level doc comment
2. Imports
3. Tool parameter structs (ProtocolParam, ListProxiesParam, CheckProxyParam, GeoipLookupParam, RemoveProxyParam)
4. ProxyPoolMcp struct definition + new() constructor + resolve_protocol() helper
5. #[tool_router] impl block — all #[tool] methods
6. #[tool_handler] impl ServerHandler — get_info()
7. #[cfg(test)] mod tests
```

**Rule**: When adding a new MCP tool, follow this order:
1. Add the parameter struct (if needed) in section 3
2. Add the `#[tool]` method in section 5
3. Add a deserialization test in section 7

---

## Naming Conventions

| Item | Convention | Example |
|------|-----------|---------|
| Parameter struct | `{Verb}{Noun}Param` | `CheckProxyParam`, `GeoipLookupParam` |
| Tool method name | `snake_case`, verb-first | `get_proxy`, `pool_status`, `geoip_lookup` |
| Tool description | Imperative sentence in `#[tool(description = "...")]` | `"Get a random working proxy from the pool"` |
| JSON output keys | `snake_case` (via `serde_json::json!`) | `"latency_ms"`, `"healthy_count"` |

---

## When to Split Files

Split `lib.rs` into sub-modules **only if**:
- Tool count exceeds ~20 (current: 10)
- Parameter structs become complex enough to warrant their own validation logic
- Shared helper functions grow beyond 3-4

Until then, keep everything in `lib.rs` for discoverability.

---

## Examples

### Adding a new tool

```rust
// 1. Parameter struct (if the tool takes parameters)
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MyNewToolParam {
    pub required_field: String,
    pub optional_field: Option<u32>,
}

// 2. Tool method inside the #[tool_router] impl block
#[tool(description = "Description of what the tool does")]
async fn my_new_tool(&self, params: Parameters<MyNewToolParam>) -> Result<String, String> {
    // delegate to proxy-core
    match self.store.some_method(params.0.required_field).await {
        Ok(result) => Ok(serde_json::to_string_pretty(&result).unwrap_or_default()),
        Err(e) => Err(format!("Error: {e}")),
    }
}

// 3. Test
#[test]
fn test_my_new_tool_param_deserialize() {
    let json = r#"{"required_field":"value","optional_field":42}"#;
    let param: MyNewToolParam = serde_json::from_str(json).unwrap();
    assert_eq!(param.required_field, "value");
    assert_eq!(param.optional_field, Some(42));
}
```
