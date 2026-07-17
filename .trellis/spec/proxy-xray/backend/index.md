# Backend Development Guidelines — proxy-xray

> Best practices for backend development in the proxy-xray crate.

---

## Overview

proxy-xray manages xray-core integration for encrypted proxy protocols (Shadowsocks, VMess, Trojan). It handles subprocess lifecycle, gRPC communication, port allocation, config generation, and background sync of pending encrypted nodes into active xray outbounds.

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Directory Structure](./directory-structure.md) | Module organization and file layout | Filled |
| ~~Database Guidelines~~ | Not applicable — no database in this crate | N/A |
| [Error Handling](./error-handling.md) | gRPC errors, reconnect patterns, subprocess supervision | Filled |
| [Quality Guidelines](./quality-guidelines.md) | Code standards, forbidden patterns, testing | Filled |
| [Logging Guidelines](./logging-guidelines.md) | Structured logging, log levels, sensitive data | Filled |
| [Active Health Demotion](./active-health-demotion.md) | Post-active revalidation, D1 demotion, shared teardown | Filled |
| [TCP Admission Precheck](./tcp-admission-precheck.md) | Cheap TCP precheck before port/xray/HTTP admission | Filled |

---

## Key Architecture Decisions

1. **Hybrid gRPC + CLI approach**: Add operations use `xray api adi/ado` CLI commands (which handle JSON-to-protobuf `TypedMessage` conversion internally). Remove operations use direct gRPC calls (only need a tag string). This avoids manually constructing `TypedMessage` wrappers in Rust.

2. **Connection state broadcasting**: `XrayClient` uses a `tokio::sync::watch` channel to broadcast connected/disconnected state. Consumers (`OutboundSync`) clone the receiver and react to state changes without polling.

3. **Exponential backoff**: Two independent backoff loops — gRPC reconnect (1s-30s) and subprocess supervisor (1s-60s). Both reset on success.

4. **In-memory port tracking**: `PortManager` uses `Arc<RwLock<HashSet<u16>>>` with sequential scan allocation. No persistence — ports are re-claimed via `claim()` on restart re-sync.

5. **Sync pause on disconnect**: `OutboundSync` skips sync cycles when gRPC is disconnected and triggers an immediate sync on reconnection.

6. **Active health demotion (D1)**: Each `sync_once` revalidates Active nodes first (budget `min(active, 32, attempt_limit)`). Demote after **2 consecutive** revalidation failures via shared teardown + registry reason `active_health_check_failed` + validation cooldown. Success resets the fail streak and merges quality onto the existing pool entry (must not wipe `encrypted_config`). Route eligibility freshness is owned by `proxy-core` (see its xray-route-eligibility code-spec).

7. **TCP admission precheck**: Before allocating a local port / pushing xray config / running HTTP validation, dial remote `host:port` with a **2s** TCP connect timeout (budget **200**/cycle). Failures only increment `SyncStats.precheck_failed` + DEBUG — they do **not** consume HTTP attempt budget, apply the 600s validation cooldown, or call `mark_failed`. Precheck success alone never marks Active.

---

**Language**: All documentation should be written in **English**.
