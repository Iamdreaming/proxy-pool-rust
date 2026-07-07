# proxy-api — Backend Development Guidelines

> REST API service for the proxy pool, built on axum.

---

## Overview

`proxy-api` exposes the proxy pool as a JSON REST API and a Prometheus metrics endpoint. It has no business logic of its own — every handler delegates to `proxy_core::store::ProxyStore` or `proxy_core::scheduler::SchedulerHandle`, then formats the result into a response struct.

---

## Guidelines Index

| Guide | Description |
|-------|-------------|
| [Directory Structure](./directory-structure.md) | Module organization and file layout |
| [Error Handling](./error-handling.md) | API error response patterns and conventions |
| [Quality Guidelines](./quality-guidelines.md) | Code standards, forbidden patterns, testing |
| [Logging Guidelines](./logging-guidelines.md) | Structured logging, log levels |
| [Settings Edit API Contract](../../proxy-core/backend/config-edit-api.md) | `GET/PUT /api/settings` response/request contract owned by `proxy-core::config` |

> **Note:** This crate has no database access and no database-guidelines.md.

---

## Key Conventions

1. **State injection** — All handlers receive `AppState` via axum `State` extractor. `AppState` is `Clone` and holds `Arc<ProxyStore>`, `Arc<AtomicUsize>`, and `SchedulerHandle`.
2. **Response structs** — Every handler returns a `Json<T>` where `T` is a `#[derive(Serialize)]` struct defined alongside the handlers in `routes.rs`.
3. **No thiserror** — Errors are handled inline per handler; no custom error enum or `IntoResponse` error type exists yet.
4. **No middleware** — CORS, tracing, etc. are wired in `proxy-server`, not here.

---

**Language**: All documentation should be written in **English**.
