# Quality Guidelines

> Code quality standards for proxy-api.

---

## Overview

proxy-api is a thin HTTP layer. Quality focus is on correct HTTP semantics, consistent response shapes, and test coverage for serialization and input parsing.

---

## Forbidden Patterns

| Pattern | Why | Instead |
|---------|-----|---------|
| `unwrap()` on store operations in handlers | Panics crash the server; store errors should return 500 | `match` with `tracing::error!` + error response |
| `unwrap_or(0)` on store `count()` calls | Silently hides store failures | `match` with 500 on `Err` |
| Returning 200 status code on error | Client cannot detect failure | Return appropriate 4xx/5xx `StatusCode` |
| Business logic in handlers | This crate is a thin API layer | Delegate to `proxy-core`; keep handlers as format-and-return |
| `log` crate macros | Project uses `tracing` | `tracing::error!`, `tracing::warn!`, etc. |
| `println!` / `eprintln!` in handlers | Not structured, not configurable | `tracing` macros |
| Mutable statics or global state | Use `AppState` via axum `State` extractor | `State(state): State<AppState>` |

---

## Required Patterns

### Handler signature

Every handler must take `State` as the first extractor:

```rust
async fn handler(State(state): State<AppState>, ...) -> impl IntoResponse
```

### Response type

Every JSON handler returns `Json<T>` where `T` derives `Serialize`. For handlers that may return error status codes, the return type should be `impl IntoResponse` and the handler should construct `(StatusCode, Json<T>)::into_response()`.

### Query parameter defaults

Use `Option<String>` for query params and apply defaults in the handler body:

```rust
let protocol_str = params.protocol.as_deref().unwrap_or("http");
let protocol = Protocol::from_str_loose(protocol_str).unwrap_or(Protocol::Http);
```

Do **not** use `#[serde(default)]` on query structs — the explicit `Option` + `unwrap_or` pattern makes defaults visible at the handler level.

### Path parameter parsing

Validate and parse path parameters in the handler. Return 400 for invalid input:

```rust
let parts: Vec<&str> = key.splitn(3, ':').collect();
if parts.len() != 3 {
    return (StatusCode::BAD_REQUEST, Json(SimpleResponse {
        status: "invalid key format, expected protocol:host:port".into(),
    })).into_response();
}
```

---

## Testing Requirements

### Minimum coverage

Every new endpoint must have:

1. **Serialization test** — Verify the response struct serializes to expected JSON.
2. **Input parsing test** — Verify query param / path param parsing for valid and invalid inputs.

### Existing test patterns

```rust
#[test]
fn test_parse_delete_key_valid() {
    let key = "http:1.2.3.4:8080";
    let parts: Vec<&str> = key.splitn(3, ':').collect();
    assert_eq!(parts.len(), 3);
}

#[test]
fn test_refresh_response_serialization() {
    let resp = RefreshResponse { status: "ok".into(), fetched: 10, ... };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"status\":\"ok\""));
}
```

### Integration tests (future)

When a test harness for `ProxyStore` is available, add integration tests that exercise the full handler pipeline via `axum::test` helpers. Current unit tests only cover parsing and serialization.

### Lint

- `cargo clippy -- -D warnings` must pass with zero warnings.
- `cargo fmt` must produce no diffs.

---

## Code Review Checklist

- [ ] Handler returns correct HTTP status codes for all branches (success, not found, bad request, internal error)
- [ ] Error branches log with `tracing::error!` before returning
- [ ] No `unwrap()` or `unwrap_or()` on fallible store operations
- [ ] Response struct derives `Serialize` and has a serialization test
- [ ] New query/path params have parsing tests for valid and invalid inputs
- [ ] No business logic in handler — all delegation goes to `proxy-core`
- [ ] Route is registered in `create_router()`
