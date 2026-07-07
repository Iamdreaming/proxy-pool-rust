# Backend Development Guidelines — proxy-sub

> Subscription source discovery and format parsing for the proxy-pool-rust workspace.

---

## Overview

`proxy-sub` is responsible for discovering subscription URLs, fetching subscription content, parsing multiple formats into `SubscriptionProxy` nodes, and routing them into the pool (basic) or pending store (encrypted). It is a pure data-ingestion crate with no API surface of its own.

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Directory Structure](./directory-structure.md) | Module organization and file layout | Filled |
| ~~Database Guidelines~~ | Not applicable — Redis access is via `PendingStore` only, documented in error-handling | N/A |
| [Error Handling](./error-handling.md) | Log-and-skip pattern in parsers and discoverers | Filled |
| [Quality Guidelines](./quality-guidelines.md) | Code standards, forbidden patterns, testing | Filled |
| [Logging Guidelines](./logging-guidelines.md) | Structured logging, log levels, sensitive data | Filled |
| [Subscription Source Report Contract](./subscription-source-report-contract.md) | Preview/apply report fields, recommendations, and write gates | Filled |

---

## Key Concepts

1. **Two trait hierarchies**: `Discover` (finds URLs) and `Parser` (parses content). Both are `Send + Sync` with `async_trait` on `Discover`.
2. **SubscriptionProxy enum**: Distinguishes `Basic` (directly usable) from encrypted variants (`Shadowsocks`, `Vmess`, `Trojan`) and `Unknown`.
3. **Auto-detection parser chain**: V2Ray JSON -> Clash YAML -> Base64 URI -> Surge. First match wins.
4. **PendingStore**: Redis ZSets keyed by `pending:encrypted:{protocol_label}`, scored by Unix timestamp.
5. **Log-and-skip**: Errors in parsers and discoverers are logged and the entry is skipped; never propagated to crash the refresh loop.

---

**Language**: All documentation is written in **English**.
