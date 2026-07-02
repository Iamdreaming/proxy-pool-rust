# Error Handling

> How errors are handled in proxy-api.

---

## Overview

proxy-api currently has **inconsistent** error handling across handlers. Some return empty/default JSON on error (masking failures), while `delete_proxy` properly uses HTTP status codes. This document records the current state and the target pattern.

---

## Current State (as of codebase analysis)

### Pattern A — Silent fallback (most handlers)

Store errors are logged with `tracing::error!` but the client receives a 200 with empty/default data:

```rust
// list_proxies — returns empty list on error
Err(e) => {
    tracing::error!("list_proxies error: {e}");
    Json(ProxiesResponse {
        protocol: protocol_str.to_string(),
        count: 0,
        proxies: vec![],
    })
}

// get_random_proxy / get_best_proxy — returns null on error
Err(e) => {
    tracing::error!("get_random_proxy error: {e}");
    Json(None)
}

// status — uses unwrap_or(0), no error logged at all
let http_count = state.store.count(Protocol::Http).await.unwrap_or(0);
```

### Pattern B — Proper HTTP status codes (delete_proxy only)

```rust
// delete_proxy — returns appropriate StatusCode + JSON body
Ok(true)  => (StatusCode::OK, Json(SimpleResponse { status: "ok".into() })).into_response(),
Ok(false) => (StatusCode::NOT_FOUND, Json(SimpleResponse { status: "proxy not found".into() })).into_response(),
Err(e)    => (StatusCode::INTERNAL_SERVER_ERROR, Json(SimpleResponse { status: format!("error: {e}") })).into_response(),
```

### Pattern C — Error in response body (refresh_pool)

```rust
// refresh_pool — always returns 200, error message in "status" field
Err(e) => Json(RefreshResponse {
    status: format!("error: {e}"),
    fetched: 0, validated: 0, stored: 0, errors: 0,
})
```

---

## Target Pattern

All handlers should follow the `delete_proxy` pattern: return the correct HTTP status code with a structured JSON error body.

### Standard error response shape

Use `SimpleResponse` for error cases:

```rust
(StatusCode::INTERNAL_SERVER_ERROR, Json(SimpleResponse {
    status: "internal error".into(),
}))
```

### Status code mapping

| Condition | HTTP Status | Example |
|-----------|-------------|---------|
| Store operation fails | 500 Internal Server Error | `store.count()` returns `Err` |
| Resource not found | 404 Not Found | `store.remove()` returns `Ok(false)` |
| Invalid input | 400 Bad Request | Malformed key, bad protocol |
| Success | 200 OK | Normal response |
| Accepted (async) | 202 Accepted | If refresh is made async in future |

### Migration priority

1. **`status` handler** — Replace `unwrap_or(0)` with proper error handling and 500 on store failure.
2. **`list_proxies`** — Return 500 instead of empty list on store error.
3. **`get_random_proxy` / `get_best_proxy`** — Return 500 on store error; 200 with `null` is acceptable for "no proxy available" (`Ok(None)`).
4. **`refresh_pool`** — Return 500 on scheduler error instead of embedding error in 200 response.
5. **`metrics`** — Same as `status`.

---

## No Custom Error Enum (Yet)

proxy-api does not define a `thiserror` error type or an axum `IntoResponse` error implementation. This is acceptable for the current size. If error handling is unified, introduce:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("store error: {0}")]
    Store(#[from] proxy_core::store::StoreError),
    #[error("invalid request: {0}")]
    BadRequest(String),
    #[error("not found")]
    NotFound,
}

impl IntoResponse for ApiError { /* map to status codes */ }
```

This is optional — the inline `(StatusCode, Json<SimpleResponse>)` pattern is sufficient for now.

---

## Common Mistakes

1. **Returning 200 on error** — Clients cannot distinguish success from failure. Always use the correct HTTP status code.
2. **Exposing internal error messages** — `format!("error: {e}")` leaks implementation details. Use a generic message in production, log the full error server-side.
3. **Swallowing errors with `unwrap_or`** — `unwrap_or(0)` silently hides store failures. Prefer `match` with explicit error logging and a 500 response.
4. **IPv6 in delete key** — The key format `protocol:host:port` uses `:` as delimiter, which breaks for IPv6 addresses. This is a known limitation documented in tests but not handled in code.
