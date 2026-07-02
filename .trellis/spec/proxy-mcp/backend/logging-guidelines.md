# Logging Guidelines — proxy-mcp

> How logging is done in the MCP Server crate.

---

## Overview

`proxy-mcp` uses the `tracing` crate for all logging, consistent with the workspace convention. However, this crate logs **very sparingly** by design — it is a thin adapter and most meaningful events originate in `proxy-core`.

---

## Log Levels

| Level | When to Use | Example in This Crate |
|-------|-------------|----------------------|
| `error` | Never (currently) — errors are returned to the MCP client as `Err` or JSON | — |
| `warn` | Unexpected conditions that don't break the tool | Unresolved protocol string falling back to `Http` (currently silent; add only if debugging) |
| `info` | Tool invocations (optional, see below) | `"MCP tool get_proxy called with protocol=http"` |
| `debug` | Detailed parameter/output tracing for development | `"get_proxy result: Ok(proxy count=1)"` |
| `trace` | Not used in this crate | — |

---

## When to Add Logging

### General Rule: Prefer Silence

MCP tools are called frequently by LLM clients. Adding `info!` to every tool invocation creates noisy logs. Follow these guidelines:

| Scenario | Action |
|----------|--------|
| Normal tool call (get, list, status) | No log — return value is the observability |
| Tool call with error result | `tracing::warn!` with tool name and error summary |
| Destructive action (remove_proxy) | `tracing::info!` with proxy identifier |
| Expensive action (refresh_pool, check_proxy) | `tracing::info!` at start, optional `debug!` on completion |
| Feature-not-configured (WARP, GeoIP None) | No log — the plain string response is sufficient |

### Current State

The crate currently has **zero** `tracing` calls. This is acceptable because:

1. `proxy-core` already logs all significant events (store operations, scheduler cycles, validation results)
2. MCP tool errors are returned to the LLM client, which is the primary consumer
3. The `proxy-server` crate can add request-level logging at the transport layer

### When to Break Silence

Add logging when:
- A tool returns `Err` from a `Result<String, String>` method — log the error at `warn` level
- A destructive operation succeeds (e.g., `remove_proxy`) — log at `info` level
- Debugging a specific tool's behavior — add temporary `debug!` calls, remove before merge

---

## Structured Logging

### Format

When adding logs, use structured fields for machine-parseable output:

```rust
// BAD — string interpolation
tracing::info!("proxy removed: {}:{}", host, port);

// GOOD — structured fields
tracing::info!(tool = "remove_proxy", host = %host, port = port, "proxy removed");
```

### Required Fields

| Field | When to Include | Example |
|-------|----------------|---------|
| `tool` | Always | `"get_proxy"`, `"refresh_pool"` |
| `host` | When the tool operates on a specific proxy | `"1.2.3.4"` |
| `port` | When the tool operates on a specific proxy | `8080` |
| `protocol` | When relevant | `"socks5"` |
| `error` | On error paths | `"Redis connection refused"` |

---

## What NOT to Log

| Data | Why Not |
|------|---------|
| Full proxy list contents | Large output, not actionable in logs |
| GeoIP lookup response bodies | Redundant — returned to client |
| MCP JSON-RPC request/response frames | rmcp handles this internally |
| Proxy passwords/credentials | Security risk (not applicable yet, but rule stands) |
| Internal store ZSet scores | Implementation detail of `proxy-core` |

---

## Integration with proxy-core Logging

`proxy-core` emits the meaningful events this crate benefits from:

| Event | Source | Log Level |
|-------|--------|-----------|
| Circuit breaker tripped | `proxy_core::store` | `info` |
| Proxy validation result | `proxy_core::validator` | `debug` |
| Scheduler cycle complete | `proxy_core::scheduler` | `info` |
| GeoIP database loaded | `proxy_core::geoip` | `info` |
| Store operation failed | `proxy_core::store` | `warn` |

When debugging MCP tool behavior, check `proxy-core` logs first — the root cause usually lives there.

---

## Common Mistakes

| Mistake | Why It's Wrong | Fix |
|---------|---------------|-----|
| Adding `info!` to every tool call | Creates log noise on every LLM request | Only log destructive or error cases |
| Using `log` crate macros | Workspace convention is `tracing` | Use `tracing::info!`, `tracing::warn!`, etc. |
| Logging full JSON tool output | Redundant — the LLM client receives it | Log metadata (tool name, param summary), not output |
| Logging at `error` level for tool failures | Tool errors are expected operational events, not system errors | Use `warn` level; `error` is for system-level failures |
