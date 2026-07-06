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

## Scenario: Environment-Gated `update_service`

### 1. Scope / Trigger

- Trigger: `update_service` can touch `/var/run/docker.sock` and trigger Watchtower to recreate the running service.
- This is an infra integration and must be explicit, auditable, and disabled by default outside managed deployment wiring.

### 2. Signatures

- MCP tool: `async fn update_service(&self) -> String`
- Config helper: `UpdateServiceConfig::from_env() -> UpdateServiceConfig`
- Docker helpers: `docker_api_get(socket_path, path)`, `docker_api_post(socket_path, path)`

### 3. Contracts

Environment variables:

| Key | Required | Meaning |
|-----|:---:|---------|
| `PROXY_POOL_UPDATE_ENABLED` | Yes for updates | Must be one of `1`, `true`, `yes`, or `on` to permit any Docker/Watchtower action |
| `PROXY_POOL_UPDATE_DOCKER_SOCKET` | Optional | Docker Unix socket path, defaults to `/var/run/docker.sock` |
| `PROXY_POOL_UPDATE_CONTAINER` | Optional | Container inspected before update, defaults to `proxy-pool` |
| `PROXY_POOL_UPDATE_IMAGE` | Optional | Image pulled before Watchtower trigger, defaults to `ghcr.io/iamdreaming/proxy-pool-rust:latest` |
| `PROXY_POOL_UPDATE_WATCHTOWER_URL` | Optional | Watchtower HTTP API endpoint |
| `PROXY_POOL_UPDATE_TOKEN` | Yes when enabled | Bearer token sent to Watchtower |

Response contract:

- Disabled: `{"status":"disabled","required_env":"PROXY_POOL_UPDATE_ENABLED=true",...}`
- Config error: `{"status":"error","message":"PROXY_POOL_UPDATE_TOKEN must be set ..."}`
- Already current: `{"status":"already_current","previous_image_id":...,"new_image_id":...,"digest_changed":false,...}`
- Triggered: `{"status":"update_triggered","previous_image_id":...,"new_image_id":...,"new_digest":...,"digest_changed":true,...}`

### 4. Validation & Error Matrix

| Condition | Response |
|-----------|----------|
| Update switch absent or false | `status=disabled`; do not touch Docker socket |
| Token missing while enabled | `status=error`; do not touch Docker socket |
| Current container inspect fails | `status=error`, message starts `failed to inspect container` |
| Image pull fails | `status=error`, message starts `docker pull failed` |
| Pulled image inspect fails | `status=error`, message starts `failed to inspect pulled image` |
| Image ID unchanged | `status=already_current`; do not call Watchtower |
| Watchtower non-2xx or unreachable | `status=error` with old/new image identity fields |

### 5. Good/Base/Bad Cases

- Good: managed compose sets `PROXY_POOL_UPDATE_ENABLED=true` and matching Watchtower token, then `update_service` pulls the image and triggers Watchtower only when image ID changed.
- Base: local dev environment leaves the switch unset; the tool returns `disabled`.
- Bad: enabled without token; the tool returns `error` before any Docker call.

### 6. Tests Required

- Unit test `UpdateServiceConfig` defaults to disabled.
- Unit test truthy bool parsing.
- Unit test image ref splitting handles registry ports.
- Unit test image ID / identity comparison.
- Integration verification, when a deployed target is available: call MCP `update_service`, then poll `/api/status.git_hash`.

### 7. Wrong vs Correct

#### Wrong

```rust
let socket_path = "/var/run/docker.sock";
let watchtower_url = "http://watchtower-proxy-pool:8080/v1/update";
client.post(watchtower_url).header("Authorization", "Bearer proxy-pool-update");
```

This hard-codes a production mutation path and secret-like token into the binary.

#### Correct

```rust
let config = UpdateServiceConfig::from_env();
if !config.enabled {
    return disabled_json();
}
let Some(token) = config.watchtower_token.as_deref() else {
    return token_error_json();
};
```

All mutation paths are explicitly enabled and configured by deployment wiring.
