# Logging Guidelines

> How logging is done in proxy-api.

---

## Overview

proxy-api uses `tracing` for all structured logging. The crate does not configure its own subscriber — that is done by `proxy-server` at startup. Handlers should only emit log messages; they should never configure or install subscribers.

---

## Log Levels

| Level | When to use | Example |
|-------|-------------|---------|
| `error!` | Store or scheduler operations fail in a handler | `tracing::error!("list_proxies error: {e}")` |
| `warn!` | Degraded but recoverable state (not currently used) | Unrecognized protocol defaulting to Http |
| `info!` | Not currently used in handlers — request logging is handled by tower-http trace middleware in proxy-server | — |
| `debug!` | Detailed request/response data for development | Query params received, response counts |
| `trace!` | Per-request lifecycle details (not currently used) | — |

### Current usage

The codebase currently only uses `tracing::error!`. This is sufficient for the current size. Adding `debug!` for request parameters is encouraged but not required.

---

## Structured Logging

### Current pattern

Errors are logged with the error message interpolated:

```rust
tracing::error!("list_proxies error: {e}");
tracing::error!("get_random_proxy error: {e}");
```

### Preferred pattern (for new code)

Use structured fields instead of format interpolation for machine-parseable logs:

```rust
tracing::error!(error = %e, handler = "list_proxies", "store operation failed");
```

This allows log aggregation tools to filter by `handler` or `error` fields.

---

## What to Log

| Event | Level | Fields |
|-------|-------|--------|
| Store operation failure | `error` | `error`, `handler`, `protocol` (if relevant) |
| Scheduler refresh failure | `error` | `error` |
| Invalid client input (bad key format, bad protocol) | `warn` | `handler`, `input` |
| Request parameters received | `debug` | `handler`, `protocol`, `limit` |

---

## What NOT to Log

| Data | Why |
|------|-----|
| Full proxy IP:port lists | High cardinality, can be large; log counts instead |
| Internal error details in client responses | Leaks implementation info; log server-side, return generic message to client |
| Request headers (auth tokens, cookies) | Security risk |
| Full request bodies | May contain sensitive data |

---

## Common Mistakes

1. **Logging and then returning empty 200** — The current code logs errors but still returns 200 to the client. The log tells the operator something is wrong, but the client has no idea. Fix by returning proper 5xx status codes.
2. **`println!` instead of `tracing`** — Never use print statements. They bypass the structured logging pipeline and cannot be filtered or redirected.
3. **Logging in a loop** — If a handler processes multiple items, log a summary (count) rather than one line per item.
4. **Over-logging at INFO** — Request-level logging is handled by tower-http middleware in proxy-server. Individual handlers should not emit INFO for every request.
