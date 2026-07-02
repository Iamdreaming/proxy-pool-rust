# Directory Structure — proxy-sub

> How the subscription discovery and parsing crate is organized.

---

## Overview

`proxy-sub` follows a clean separation between discovery (finding URLs), parsing (interpreting content), and the pipeline that orchestrates them. Each responsibility maps to a dedicated module.

---

## Directory Layout

```
crates/proxy-sub/
├── Cargo.toml
├── src/
│   ├── lib.rs                  # Crate root — re-exports public modules
│   ├── models.rs               # SubscriptionProxy enum, ParsedProxy struct
│   ├── convert.rs              # to_proxy(), partition() — SubscriptionProxy -> Proxy
│   ├── pending.rs              # PendingStore — Redis ZSets for encrypted nodes
│   ├── refresh.rs              # build_discoverers(), run_refresh_cycle(), subscription_refresh_loop()
│   ├── discover/
│   │   ├── mod.rs              # Discover trait (async_trait), NullDiscover test stub
│   │   ├── static_url.rs       # StaticUrlDiscover — pre-configured URL list
│   │   ├── github_search.rs    # GitHubSearchDiscover — GitHub Search API (repo + code)
│   │   └── aggregator.rs       # AggregatorDiscover — text/json/yaml URL lists
│   ├── parser/
│   │   ├── mod.rs              # Parser trait, builtin_parsers(), parse_subscription() (auto-detect)
│   │   ├── v2ray_json.rs       # V2rayJsonParser — V2Ray/Xray JSON outbounds
│   │   ├── clash.rs            # ClashParser — Clash/Mihomo YAML proxies array
│   │   ├── base64_uri.rs       # Base64UriParser — base64 blob or per-line URIs (ss/vmess/trojan/socks5/http)
│   │   └── surge.rs            # SurgeParser — Surge proxy list lines
│   └── source/
│       ├── mod.rs              # SubscriptionSource — HTTP fetch + cache integration
│       └── cache.rs            # ContentCache — in-memory TTL cache (HashMap<String, (String, Instant)>)
└── tests/
    └── fixtures/               # Sample subscription files for parser tests
        ├── v2ray_sample.json
        ├── clash_sample.yaml
        ├── base64_sample.txt
        ├── base64_blob.txt
        ├── surge_sample.txt
        └── mixed_invalid.txt
```

---

## Module Organization

### Trait Modules (`discover/`, `parser/`)

Each trait module follows the same pattern:
- `mod.rs` — trait definition + re-exports of implementations
- One file per implementation, named after the concrete type in `snake_case`

New discoverers or parsers must:
1. Add a new file under the appropriate directory
2. Implement the trait (`Discover` or `Parser`)
3. Re-export from `mod.rs`
4. Register in the factory function (`build_discoverers()` for discoverers, `builtin_parsers()` for parsers)

### Models Module (`models.rs`)

Single file containing the `SubscriptionProxy` enum and `ParsedProxy` struct. All protocol variants are represented as enum variants rather than trait objects — this keeps pattern matching exhaustive and avoids dynamic dispatch overhead.

### Pipeline Module (`refresh.rs`)

Orchestration layer that ties discoverers, source, parser, and storage together. Contains the refresh loop but no business logic of its own.

### Storage Module (`pending.rs`)

Redis-backed store for encrypted nodes. Uses `anyhow::Result` for error propagation since Redis failures are meaningful to the caller.

### Source Module (`source/`)

Two-file module: `SubscriptionSource` in `mod.rs` wraps HTTP client + cache; `cache.rs` is a standalone `ContentCache` with TTL-based lazy eviction.

---

## Naming Conventions

| Item | Convention | Example |
|------|-----------|---------|
| Trait | Noun | `Discover`, `Parser` |
| Trait impl struct | PascalCase + suffix describing source/format | `StaticUrlDiscover`, `ClashParser` |
| Config struct | PascalCase + `Config` suffix | `GitHubSearchConfig`, `AggregatorConfig` |
| Helper functions (private) | snake_case | `parse_ss_query`, `extract_stream_settings`, `github_to_raw_url` |
| Fixture files | `{format}_sample.{ext}` or `{format}_blob.{ext}` | `v2ray_sample.json`, `base64_blob.txt` |
| Redis keys | `pending:encrypted:{protocol_label}` | `pending:encrypted:vmess` |

---

## Dependency Direction

```
refresh.rs
  ├── discover/    (trait + impls)
  ├── source/      (HTTP fetch + cache)
  ├── parser/      (trait + impls)
  ├── convert.rs   (partition logic)
  ├── pending.rs   (Redis store)
  └── proxy-core   (Proxy, Protocol, ProxyStore, SubscriptionConfig)

models.rs ← parser/, convert/, pending/ (all depend on SubscriptionProxy)
parser/   ← models.rs, proxy-core (Protocol)
convert   ← models.rs, proxy-core (Proxy)
```

No circular dependencies. `parser/` never depends on `discover/` or `source/`.

---

## Examples

Well-organized modules to emulate:
- `parser/clash.rs` — clean `Parser` trait implementation with partial serde deserialization
- `discover/aggregator.rs` — single discoverer with multiple format support in private helper functions
- `source/cache.rs` — self-contained, testable, no external dependencies beyond `std`
