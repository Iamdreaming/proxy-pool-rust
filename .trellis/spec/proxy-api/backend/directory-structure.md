# Directory Structure

> How proxy-api code is organized.

---

## Overview

`proxy-api` is a thin crate with two source files: `lib.rs` for shared state and app construction, `routes.rs` for all route definitions, handlers, query/response types, and tests.

---

## Directory Layout

```
crates/proxy-api/
├── Cargo.toml
└── src/
    ├── lib.rs      — AppState, create_app()
    └── routes.rs   — Router, handlers, query/response types, unit tests
```

---

## Module Organization

### `lib.rs` — App shell

- Defines `AppState` (shared state struct).
- Exports `create_app(state) -> Router` which assembles the axum router.
- Declares `mod routes` — no other submodules.

### `routes.rs` — All HTTP concerns

Contains, in order:

1. **Query param structs** — `ProxyQuery`, `ProxyProtocolQuery`, `DeleteProxyPath`
2. **Response structs** — `StatusResponse`, `PoolStatus`, `ProxiesResponse`, `SimpleResponse`, `RefreshResponse`; xray lifecycle status serializes shared `proxy_core::xray_status::XrayStatusSnapshot`
3. **Route builder** — `create_router() -> Router<AppState>`
4. **Handlers** — one `async fn` per route
5. **Tests** — `#[cfg(test)] mod tests` at the bottom

### When to split

If `routes.rs` grows beyond ~400 lines, split by domain:

```
src/
├── lib.rs
├── routes/
│   ├── mod.rs          — create_router(), re-exports
│   ├── proxy.rs        — /api/proxy/*, /api/proxies/*
│   ├── status.rs       — /api/status, /api/metrics, /api/xray/status
│   └── types.rs        — shared query/response structs
```

Do **not** split until the file is genuinely hard to navigate. The current two-file layout is intentional — the crate is small.

---

## Naming Conventions

| Item | Convention | Example |
|------|-----------|---------|
| Handler functions | `snake_case`, named after the route action | `status`, `list_proxies`, `delete_proxy` |
| Response structs | `PascalCase` + `Response` suffix | `StatusResponse`, `RefreshResponse` |
| Query param structs | `PascalCase` + `Query` suffix | `ProxyQuery`, `ProxyProtocolQuery` |
| Route paths | `/api/{resource}` plural for collections, singular for single-item | `/api/proxies`, `/api/proxy/random` |

---

## Adding a New Endpoint

1. Define the response struct in `routes.rs` (derive `Serialize`).
2. Define a query struct if the endpoint takes query params (derive `Deserialize`).
3. Write the handler function — must take `State(state): State<AppState>` as first argument.
4. Register the route in `create_router()`.
5. Add a serialization test in the `tests` module.
