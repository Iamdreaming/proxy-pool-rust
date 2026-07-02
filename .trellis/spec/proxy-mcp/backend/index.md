# Backend Development Guidelines — proxy-mcp

> MCP Server crate exposing proxy pool management tools to LLMs via the rmcp protocol.

---

## Overview

`proxy-mcp` is a thin adapter layer: it wraps `proxy-core` services (store, scheduler, GeoIP, WARP balancer) behind MCP tool definitions so that LLM clients can query and manage the proxy pool. The crate contains a single `ProxyPoolMcp` struct that implements `rmcp`'s `ServerHandler` trait, with all tool methods registered via the `#[tool_router]` / `#[tool_handler]` macro pipeline.

**Key design principle**: This crate does **not** contain business logic. It delegates everything to `proxy-core` and focuses on parameter deserialization, protocol resolution, and JSON output formatting.

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Directory Structure](./directory-structure.md) | Module organization and file layout | Done |
| [Error Handling](./error-handling.md) | MCP tool error patterns | Done |
| [Quality Guidelines](./quality-guidelines.md) | Code standards, forbidden patterns | Done |
| [Logging Guidelines](./logging-guidelines.md) | Structured logging, log levels | Done |

> No `database-guidelines.md` — this crate has no direct database access; all persistence goes through `proxy-core::store::ProxyStore`.

---

## Crate Role in Workspace

```
proxy-core  ←  proxy-mcp  →  rmcp (MCP protocol)
     ↑                ↑
  store/scheduler   LLM clients (via stdio / Streamable HTTP)
  geoip/warp
```

- **Upstream dependency**: `proxy-core` (models, store, scheduler, geoip, warp, validator)
- **Downstream consumers**: `proxy-server` (assembles and starts the MCP server)
- **Protocol**: rmcp (Rust MCP SDK) — `#[tool]`, `Parameters<T>`, `ToolRouter`, `ServerHandler`

---

## How to Use These Guidelines

1. Document your project's **actual conventions** (not ideals)
2. Include **code examples** from your codebase
3. List **forbidden patterns** and why
4. Add **common mistakes** your team has made

---

**Language**: All documentation should be written in **English**.
