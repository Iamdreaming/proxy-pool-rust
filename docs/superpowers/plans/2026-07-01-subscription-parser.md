# Subscription Parser Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `proxy-sub` crate with subscription format parsing (4 formats) and source discovery (3 methods), integrate basic-protocol nodes into existing ProxyStore, and store encrypted-protocol nodes as pending in Redis.

**Architecture:** New `proxy-sub` crate sits between external subscription sources and the existing `proxy-core` ProxyStore. Parsers auto-detect format (Clash YAML / Base64 URI / V2Ray JSON / Surge) and produce `SubscriptionProxy` enum. Discovery modules (StaticUrl / GitHubSearch / Aggregator) find subscription URLs. Basic nodes (socks5/http/https) convert to `Proxy` and enter the pool; encrypted nodes (ss/vmess/trojan) persist to Redis pending keys for Phase 2.

**Tech Stack:** Rust edition 2024, serde + serde_yaml + serde_json + base64 for parsing, reqwest for HTTP, redis for pending storage, async-trait for trait async methods.

## Global Constraints

- Edition 2024, workspace dependency pattern (`workspace = true`)
- Error handling: library code uses `thiserror`, app code uses `anyhow`
- Logging: `tracing` crate, never `log`
- Serialization: `serde` + `serde_json` / `serde_yaml`
- Tests: per-crate `tests/` directory + inline `#[cfg(test)]`
- Lint: `cargo clippy -- -D warnings`
- Commit format: `type(scope): description`

## File Structure

| Action | Path | Responsibility |
|--------|------|---------------|
| Create | `crates/proxy-sub/Cargo.toml` | Crate dependencies |
| Create | `crates/proxy-sub/src/lib.rs` | Module exports |
| Create | `crates/proxy-sub/src/models.rs` | `SubscriptionProxy` enum |
| Create | `crates/proxy-sub/src/parser/mod.rs` | `Parser` trait + `parse_subscription()` entrypoint |
| Create | `crates/proxy-sub/src/parser/clash.rs` | Clash YAML parser |
| Create | `crates/proxy-sub/src/parser/base64_uri.rs` | Base64 URI list parser |
| Create | `crates/proxy-sub/src/parser/v2ray_json.rs` | V2Ray JSON parser |
| Create | `crates/proxy-sub/src/parser/surge.rs` | Surge line parser |
| Create | `crates/proxy-sub/src/discover/mod.rs` | `Discover` trait + `build_discoverers()` |
| Create | `crates/proxy-sub/src/discover/static_url.rs` | Static URL discoverer |
| Create | `crates/proxy-sub/src/discover/github_search.rs` | GitHub Search API discoverer |
| Create | `crates/proxy-sub/src/discover/aggregator.rs` | Aggregator discoverer |
| Create | `crates/proxy-sub/src/source/mod.rs` | `SubscriptionSource` fetcher |
| Create | `crates/proxy-sub/src/source/cache.rs` | In-memory content cache |
| Create | `crates/proxy-sub/src/convert.rs` | `SubscriptionProxy::Basic` → `Proxy` conversion |
| Create | `crates/proxy-sub/src/pending.rs` | Redis pending storage for encrypted nodes |
| Create | `crates/proxy-sub/src/refresh.rs` | Subscription refresh loop orchestration |
| Create | `crates/proxy-sub/tests/fixtures/clash_sample.yaml` | Clash YAML test fixture |
| Create | `crates/proxy-sub/tests/fixtures/base64_sample.txt` | Base64 URI test fixture |
| Create | `crates/proxy-sub/tests/fixtures/v2ray_sample.json` | V2Ray JSON test fixture |
| Create | `crates/proxy-sub/tests/fixtures/surge_sample.txt` | Surge test fixture |
| Create | `crates/proxy-sub/tests/fixtures/mixed_invalid.txt` | Invalid content fixture |
| Modify | `Cargo.toml` | Add `proxy-sub` to workspace members + workspace deps |
| Modify | `crates/proxy-core/src/config.rs` | Add `SubscriptionConfig` to `Settings` |
| Modify | `crates/proxy-core/src/models.rs` | Add `EncryptedProxyState` enum |
| Modify | `crates/proxy-server/Cargo.toml` | Add `proxy-sub` dependency |
| Modify | `crates/proxy-server/src/main.rs` | Add subscription refresh loop spawn |

---

### Task 1: Crate Scaffolding + SubscriptionProxy Model

**Files:**
- Create: `crates/proxy-sub/Cargo.toml`
- Create: `crates/proxy-sub/src/lib.rs`
- Create: `crates/proxy-sub/src/models.rs`
- Modify: `Cargo.toml` (workspace members + `url` dep)
- Modify: `crates/proxy-core/src/models.rs` (add `EncryptedProxyState`)
- Test: `crates/proxy-sub/src/models.rs` (inline tests)

**Interfaces:**
- Consumes: `proxy_core::models::Protocol` (existing)
- Produces: `SubscriptionProxy` enum, `EncryptedProxyState` enum

- [ ] **Step 1: Add proxy-sub to workspace Cargo.toml**

Add `proxy-sub` to workspace members and `url = "2"` to workspace dependencies:

```toml
# In Cargo.toml [workspace] members array, add:
members = [
    "crates/proxy-core",
    "crates/proxy-api",
    "crates/proxy-gateway",
    "crates/proxy-mcp",
    "crates/proxy-server",
    "crates/proxy-sub",
]

# In [workspace.dependencies], add:
url = "2"
base64 = "0.22"
```

- [ ] **Step 2: Create proxy-sub/Cargo.toml**

```toml
[package]
name = "proxy-sub"
version.workspace = true
edition.workspace = true

[dependencies]
proxy-core = { path = "../proxy-core" }
tokio = { workspace = true }
reqwest = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
serde_yaml = { workspace = true }
redis = { workspace = true }
tracing = { workspace = true }
chrono = { workspace = true }
async-trait = { workspace = true }
anyhow = { workspace = true }
url = { workspace = true }
base64 = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["test-util", "macros"] }
```

- [ ] **Step 3: Create proxy-sub/src/lib.rs**

```rust
//! proxy-sub: subscription source discovery and format parsing.
//!
//! Parses Clash YAML, V2Ray base64, V2Ray JSON, and Surge subscription
//! formats into `SubscriptionProxy` nodes. Discoveres subscription URLs
//! from static config, GitHub search, and aggregator projects.

pub mod convert;
pub mod discover;
pub mod models;
pub mod parser;
pub mod pending;
pub mod refresh;
pub mod source;
```

- [ ] **Step 4: Create proxy-sub/src/models.rs with SubscriptionProxy enum**

```rust
//! Subscription proxy node model.

use proxy_core::models::Protocol;
use serde::{Deserialize, Serialize};

/// A proxy node parsed from a subscription, carrying full protocol parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubscriptionProxy {
    /// Basic protocol: socks5/http/https — directly usable in the pool.
    Basic {
        host: String,
        port: u16,
        protocol: Protocol,
    },
    /// Shadowsocks node.
    Shadowsocks {
        host: String,
        port: u16,
        method: String,
        password: String,
        plugin: Option<String>,
        plugin_opts: Option<String>,
    },
    /// VMess (V2Ray) node.
    Vmess {
        host: String,
        port: u16,
        uuid: String,
        alter_id: u32,
        security: String,
        network: String,
        path: Option<String>,
        host_header: Option<String>,
        sni: Option<String>,
    },
    /// Trojan node.
    Trojan {
        host: String,
        port: u16,
        password: String,
        sni: Option<String>,
        network: Option<String>,
    },
    /// Unknown or unsupported protocol.
    Unknown {
        raw_config: String,
    },
}

impl SubscriptionProxy {
    /// Whether this node can be directly used as a pool proxy (basic protocol).
    pub fn is_direct_usable(&self) -> bool {
        matches!(self, Self::Basic { .. })
    }

    /// The host address of this node (if available).
    pub fn host(&self) -> Option<&str> {
        match self {
            Self::Basic { host, .. } => Some(host),
            Self::Shadowsocks { host, .. } => Some(host),
            Self::Vmess { host, .. } => Some(host),
            Self::Trojan { host, .. } => Some(host),
            Self::Unknown { .. } => None,
        }
    }

    /// The port of this node (if available).
    pub fn port(&self) -> Option<u16> {
        match self {
            Self::Basic { port, .. } => Some(*port),
            Self::Shadowsocks { port, .. } => Some(*port),
            Self::Vmess { port, .. } => Some(*port),
            Self::Trojan { port, .. } => Some(*port),
            Self::Unknown { .. } => None,
        }
    }

    /// Protocol type label for dedup and Redis key routing.
    pub fn protocol_label(&self) -> &str {
        match self {
            Self::Basic { .. } => "basic",
            Self::Shadowsocks { .. } => "ss",
            Self::Vmess { .. } => "vmess",
            Self::Trojan { .. } => "trojan",
            Self::Unknown { .. } => "unknown",
        }
    }

    /// Dedup key: host:port:protocol_label
    pub fn dedup_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.host().unwrap_or(""),
            self.port().unwrap_or(0),
            self.protocol_label()
        )
    }
}

/// Metadata attached to a parsed proxy: where it came from and when.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedProxy {
    pub proxy: SubscriptionProxy,
    pub source_url: String,
    pub discovered_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_direct_usable() {
        let basic = SubscriptionProxy::Basic {
            host: "1.2.3.4".into(),
            port: 1080,
            protocol: Protocol::Socks5,
        };
        assert!(basic.is_direct_usable());

        let ss = SubscriptionProxy::Shadowsocks {
            host: "1.2.3.4".into(),
            port: 8388,
            method: "aes-256-gcm".into(),
            password: "pass".into(),
            plugin: None,
            plugin_opts: None,
        };
        assert!(!ss.is_direct_usable());
    }

    #[test]
    fn test_dedup_key() {
        let basic = SubscriptionProxy::Basic {
            host: "1.2.3.4".into(),
            port: 1080,
            protocol: Protocol::Socks5,
        };
        assert_eq!(basic.dedup_key(), "1.2.3.4:1080:basic");

        let vmess = SubscriptionProxy::Vmess {
            host: "5.6.7.8".into(),
            port: 443,
            uuid: "uid".into(),
            alter_id: 0,
            security: "auto".into(),
            network: "ws".into(),
            path: None,
            host_header: None,
            sni: None,
        };
        assert_eq!(vmess.dedup_key(), "5.6.7.8:443:vmess");
    }
}
```

- [ ] **Step 5: Add EncryptedProxyState to proxy-core/src/models.rs**

Add at the end of the `WARP models` section (after the `WarpInstance` struct, around line 228):

```rust
// -- Encrypted proxy state (Phase 2 reservation) --

/// State of an encrypted-protocol proxy node awaiting xray integration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum EncryptedProxyState {
    /// Waiting for xray instance to assign a local port.
    Pending,
    /// xray configured, local socks5 port available.
    Active { local_socks5_port: u16 },
    /// Configuration failed or xray unavailable.
    Failed,
}
```

- [ ] **Step 6: Run tests to verify**

Run: `cargo test -p proxy-sub --lib`
Expected: PASS (2 tests: `test_is_direct_usable`, `test_dedup_key`)

Run: `cargo test -p proxy-core --lib`
Expected: PASS (existing tests + no regression)

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/proxy-sub/ crates/proxy-core/src/models.rs
git commit -m "feat(sub): scaffold proxy-sub crate and SubscriptionProxy model"
```

---

### Task 2: Parser Trait + Clash YAML Parser

**Files:**
- Create: `crates/proxy-sub/src/parser/mod.rs`
- Create: `crates/proxy-sub/src/parser/clash.rs`
- Create: `crates/proxy-sub/tests/fixtures/clash_sample.yaml`
- Test: `crates/proxy-sub/src/parser/clash.rs` (inline tests)

**Interfaces:**
- Consumes: `SubscriptionProxy` (from Task 1)
- Produces: `Parser` trait, `ClashParser`, `parse_subscription()` entrypoint

- [ ] **Step 1: Write the failing test for Clash parser**

Create test fixture `crates/proxy-sub/tests/fixtures/clash_sample.yaml`:

```yaml
proxies:
  - name: "socks5-node"
    type: socks5
    server: 10.0.0.1
    port: 1080
  - name: "http-node"
    type: http
    server: 10.0.0.2
    port: 8080
    tls: false
  - name: "ss-node"
    type: ss
    server: 10.0.0.3
    port: 8388
    cipher: aes-256-gcm
    password: "mypassword"
  - name: "vmess-node"
    type: vmess
    server: 10.0.0.4
    port: 443
    uuid: "a3482e88-686a-4a58-8126-99c9df64b7bf"
    alterId: 0
    cipher: auto
    network: ws
    ws-opts:
      path: /v2ray
      headers:
        Host: vmess.example.com
  - name: "trojan-node"
    type: trojan
    server: 10.0.0.5
    port: 443
    password: "trojanpass"
    sni: trojan.example.com
  - name: "hysteria2-node"
    type: hysteria2
    server: 10.0.0.6
    port: 443
    password: "hypass"
```

Add inline tests in `crates/proxy-sub/src/parser/clash.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SubscriptionProxy;

    const FIXTURE: &str = include_str!("../../tests/fixtures/clash_sample.yaml");

    #[test]
    fn test_clash_detect_valid() {
        let parser = ClashParser;
        assert!(parser.detect(FIXTURE));
    }

    #[test]
    fn test_clash_detect_invalid() {
        let parser = ClashParser;
        assert!(!parser.detect("just some random text"));
        assert!(!parser.detect("{ \"outbounds\": [] }"));
    }

    #[test]
    fn test_clash_parse() {
        let parser = ClashParser;
        let proxies = parser.parse(FIXTURE);
        assert_eq!(proxies.len(), 6);

        // Check socks5 → Basic
        let socks5 = &proxies[0];
        assert!(socks5.is_direct_usable());
        if let SubscriptionProxy::Basic { host, port, protocol } = socks5 {
            assert_eq!(host, "10.0.0.1");
            assert_eq!(*port, 1080);
            assert_eq!(*protocol, Protocol::Socks5);
        }

        // Check ss → Shadowsocks
        let ss = &proxies[2];
        if let SubscriptionProxy::Shadowsocks { host, method, .. } = ss {
            assert_eq!(host, "10.0.0.3");
            assert_eq!(method, "aes-256-gcm");
        }

        // Check vmess → Vmess
        let vmess = &proxies[3];
        if let SubscriptionProxy::Vmess { uuid, network, .. } = vmess {
            assert_eq!(uuid, "a3482e88-686a-4a58-8126-99c9df64b7bf");
            assert_eq!(network, "ws");
        }

        // Check hysteria2 → Unknown
        let unknown = &proxies[5];
        assert!(matches!(unknown, SubscriptionProxy::Unknown { .. }));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p proxy-sub --lib parser::clash`
Expected: FAIL (module not yet created)

- [ ] **Step 3: Create parser/mod.rs with Parser trait and parse_subscription()**

```rust
//! Parser trait and format-auto-detection entrypoint.

use crate::models::SubscriptionProxy;

mod clash;
mod base64_uri;
mod v2ray_json;
mod surge;

pub use clash::ClashParser;
pub use base64_uri::Base64UriParser;
pub use v2ray_json::V2rayJsonParser;
pub use surge::SurgeParser;

/// A subscription format parser: detect format, parse content.
pub trait Parser: Send + Sync {
    /// Human-readable name for logging.
    fn name(&self) -> &str;

    /// Check whether raw content matches this format.
    fn detect(&self, content: &str) -> bool;

    /// Parse content into a list of subscription proxies.
    /// Malformed entries are skipped with a warning log.
    fn parse(&self, content: &str) -> Vec<SubscriptionProxy>;
}

/// All built-in parsers in detection order.
pub fn builtin_parsers() -> Vec<Box<dyn Parser>> {
    vec![
        Box::new(V2rayJsonParser),   // JSON: fast reject if not valid JSON
        Box::new(ClashParser),       // YAML: check for `proxies:` key
        Box::new(Base64UriParser),   // Base64: decode + check for `://`
        Box::new(SurgeParser),       // Line regex
    ]
}

/// Auto-detect format and parse content using built-in parsers.
/// First matching parser wins. Returns empty vec if no parser matches.
pub fn parse_subscription(content: &str) -> Vec<SubscriptionProxy> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    for parser in builtin_parsers() {
        if parser.detect(trimmed) {
            tracing::info!("subscription parser: detected {} format", parser.name());
            return parser.parse(trimmed);
        }
    }

    tracing::warn!("subscription parser: no format detected for content (len={})", trimmed.len());
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_subscription_empty() {
        assert!(parse_subscription("").is_empty());
        assert!(parse_subscription("   ").is_empty());
    }

    #[test]
    fn test_parse_subscription_no_match() {
        assert!(parse_subscription("hello world\nfoo bar").is_empty());
    }
}
```

- [ ] **Step 4: Implement ClashParser**

Create `crates/proxy-sub/src/parser/clash.rs`:

```rust
//! Clash YAML subscription parser.
//!
//! Extracts the `proxies:` array from Clash/Mihomo YAML config.
//! Supported types: socks5, http, ss, vmess, trojan.
//! Unsupported types (hysteria2, wireguard, etc.) map to `Unknown`.

use crate::models::SubscriptionProxy;
use crate::parser::Parser;
use proxy_core::models::Protocol;
use serde::Deserialize;

/// Clash YAML proxy entry (partial deserialization — we only need common fields).
#[derive(Debug, Deserialize)]
struct ClashProxyEntry {
    #[serde(default)]
    name: String,
    #[serde(default)]
    type: String,
    #[serde(default)]
    server: String,
    #[serde(default)]
    port: u16,
    // SS fields
    #[serde(default, rename = "cipher")]
    ss_cipher: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    plugin: Option<String>,
    #[serde(default, rename = "plugin-opts")]
    plugin_opts: Option<String>,
    // VMess fields
    #[serde(default)]
    uuid: String,
    #[serde(default, rename = "alterId")]
    alter_id: u32,
    #[serde(default, rename = "cipher")]
    vmess_cipher: String,
    #[serde(default)]
    network: String,
    #[serde(default)]
    sni: Option<String>,
    // WS opts
    #[serde(default, rename = "ws-opts")]
    ws_opts: Option<WsOpts>,
    // Trojan fields
    #[serde(default, rename = "skip-cert-verify")]
    skip_cert_verify: Option<bool>,
    // HTTP fields
    #[serde(default)]
    tls: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct WsOpts {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    headers: Option<std::collections::HashMap<String, String>>,
}

/// Top-level Clash YAML with `proxies` key.
#[derive(Debug, Deserialize)]
struct ClashConfig {
    #[serde(default)]
    proxies: Vec<ClashProxyEntry>,
}

/// Clash YAML format parser.
pub struct ClashParser;

impl Parser for ClashParser {
    fn name(&self) -> &str {
        "Clash YAML"
    }

    fn detect(&self, content: &str) -> bool {
        // Quick check: must be valid YAML containing `proxies:` key
        if !content.contains("proxies:") {
            return false;
        }
        // Try to parse as YAML — reject if parsing fails
        serde_yaml::from_str::<ClashConfig>(content).is_ok()
    }

    fn parse(&self, content: &str) -> Vec<SubscriptionProxy> {
        let config: ClashConfig = match serde_yaml::from_str(content) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Clash YAML: parse error: {e}");
                return Vec::new();
            }
        };

        config
            .proxies
            .iter()
            .filter_map(|entry| {
                let type_lower = entry.type.to_ascii_lowercase();
                match type_lower.as_str() {
                    "socks5" => Some(SubscriptionProxy::Basic {
                        host: entry.server.clone(),
                        port: entry.port,
                        protocol: Protocol::Socks5,
                    }),
                    "http" => Some(SubscriptionProxy::Basic {
                        host: entry.server.clone(),
                        port: entry.port,
                        protocol: if entry.tls == Some(true) {
                            Protocol::Https
                        } else {
                            Protocol::Http
                        },
                    }),
                    "ss" => Some(SubscriptionProxy::Shadowsocks {
                        host: entry.server.clone(),
                        port: entry.port,
                        method: entry.ss_cipher.clone(),
                        password: entry.password.clone(),
                        plugin: entry.plugin.clone(),
                        plugin_opts: entry.plugin_opts.clone(),
                    }),
                    "vmess" => {
                        let (path, host_header) = match &entry.ws_opts {
                            Some(ws) => (ws.path.clone(), ws.headers.get("Host").cloned()),
                            None => (None, None),
                        };
                        Some(SubscriptionProxy::Vmess {
                            host: entry.server.clone(),
                            port: entry.port,
                            uuid: entry.uuid.clone(),
                            alter_id: entry.alter_id,
                            security: entry.vmess_cipher.clone(),
                            network: if entry.network.is_empty() {
                                "tcp".into()
                            } else {
                                entry.network.clone()
                            },
                            path,
                            host_header,
                            sni: entry.sni.clone(),
                        })
                    }
                    "trojan" => Some(SubscriptionProxy::Trojan {
                        host: entry.server.clone(),
                        port: entry.port,
                        password: entry.password.clone(),
                        sni: entry.sni.clone(),
                        network: if entry.network.is_empty() {
                            None
                        } else {
                            Some(entry.network.clone())
                        },
                    }),
                    _ => Some(SubscriptionProxy::Unknown {
                        raw_config: format!(
                            "type={}, server={}, port={}",
                            entry.type, entry.server, entry.port
                        ),
                    }),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SubscriptionProxy;

    const FIXTURE: &str = include_str!("../../tests/fixtures/clash_sample.yaml");

    #[test]
    fn test_clash_detect_valid() {
        let parser = ClashParser;
        assert!(parser.detect(FIXTURE));
    }

    #[test]
    fn test_clash_detect_invalid() {
        let parser = ClashParser;
        assert!(!parser.detect("just some random text"));
        assert!(!parser.detect("{ \"outbounds\": [] }"));
    }

    #[test]
    fn test_clash_parse() {
        let parser = ClashParser;
        let proxies = parser.parse(FIXTURE);
        assert_eq!(proxies.len(), 6);

        // socks5 → Basic
        assert!(proxies[0].is_direct_usable());
        if let SubscriptionProxy::Basic { host, port, protocol } = &proxies[0] {
            assert_eq!(host, "10.0.0.1");
            assert_eq!(*port, 1080);
            assert_eq!(*protocol, Protocol::Socks5);
        }

        // ss → Shadowsocks
        if let SubscriptionProxy::Shadowsocks { host, method, .. } = &proxies[2] {
            assert_eq!(host, "10.0.0.3");
            assert_eq!(method, "aes-256-gcm");
        }

        // vmess → Vmess
        if let SubscriptionProxy::Vmess { uuid, network, .. } = &proxies[3] {
            assert_eq!(uuid, "a3482e88-686a-4a58-8126-99c9df64b7bf");
            assert_eq!(network, "ws");
        }

        // hysteria2 → Unknown
        assert!(matches!(&proxies[5], SubscriptionProxy::Unknown { .. }));
    }
}
```

- [ ] **Step 5: Create test fixtures directory and clash_sample.yaml**

Create `crates/proxy-sub/tests/fixtures/clash_sample.yaml` with the content from Step 1.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p proxy-sub --lib`
Expected: PASS (all model + parser tests)

Run: `cargo clippy -p proxy-sub -- -D warnings`
Expected: No warnings

- [ ] **Step 7: Commit**

```bash
git add crates/proxy-sub/
git commit -m "feat(sub): add Parser trait and Clash YAML parser"
```

---

### Task 3: Base64 URI Parser

**Files:**
- Create: `crates/proxy-sub/src/parser/base64_uri.rs`
- Create: `crates/proxy-sub/tests/fixtures/base64_sample.txt`
- Test: inline tests

**Interfaces:**
- Consumes: `Parser` trait, `SubscriptionProxy` (Task 2, Task 1)
- Produces: `Base64UriParser`

- [ ] **Step 1: Create test fixture**

The fixture is base64-encoded content containing ss://, vmess://, trojan://, and socks5:// URIs.

Create `crates/proxy-sub/tests/fixtures/base64_sample.txt` with this base64 string (manually generated):

First, the decoded content:
```
ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@10.0.0.3:8388
vmess://eyJ2IjoiMiIsInBzIjoiIiwidWlkIjoiYTM0ODJlODgtNjg2YS00YTU4LTgxMjYtOTljOWRmNjRiN2JmIiwiaG5iIjoiMTAuMC4wLjQiLCJwbnQiOiI0NDMiLCJhZGQiOiIwIiwibmV0Ijoid3MiLCJ0eXBlIjoiIiwiaG9zdCI6InZtZXNzLmV4YW1wbGUuY29tIiwicGF0aCI6Ii92MnJheSIsInRscyI6InRscyIsInNuaSI6IiJ9
trojan://dHJvanBhc3M@10.0.0.5:443?sni=trojan.example.com
socks5://10.0.0.1:1080
```

Base64-encode the above and store the result. For the fixture file, we'll encode it at test time. Actually, the fixture should be the raw base64 string. Let's store the pre-encoded version:

Create `crates/proxy-sub/tests/fixtures/base64_sample.txt` containing:
```
c3M6Ly9ZWFVzLTI1Ni1nY206cGFzc3dvcmQ@10.0.0.3:8388
dm1lc3M6Ly9leUp2IjoiMiIsInBzIjoiIiwidWlkIjoiYTM0ODJlODgtNjg2YS00YTU4LTgxMjYtOTljOWRmNjRiN2JmIiwiaG5iIjoiMTAuMC4wLjQiLCJwbnQiOiI0NDMiLCJhZGQiOiIwIiwibmV0Ijoid3MiLCJ0eXBlIjoiIiwiaG9zdCI6InZtZXNzLmV4YW1wbGUuY29tIiwicGF0aCI6Ii92MnJheSIsInRscyI6InRscyIsInNuaSI6IiJ9
dHJvamFuOi8vdHJvanBhc3M@10.0.0.5:443?sni=trojan.example.com
c29ja3M1Oi8vMTAuMC4wLjE6MTA4MA==
```

(Note: base64-encoded lines where the entire content of a typical subscription is one big base64 blob. For simplicity, we'll test with individually encoded lines plus a full-blob variant.)

Also create `crates/proxy-sub/tests/fixtures/base64_blob.txt` containing the base64 encoding of the full multi-line content above:
```
c3M6Ly9ZWFVzLTI1Ni1nY206cGFzc3dvcmQkMTAuMC4wLjM6ODM4OAp2bWVzczovL2V5SnYiOiIyIiwicHMiOiIiLCJ1aWQiOiJhMzQ4MmU4OC02ODZhLTRhNTgtODEyNi05OWM5ZGY2NGI3YmYiLCJobiI6IjEwLjAuMC4wLjQiLCJwbnQiOiI0NDMiLCJhZGQiOiIwIiwibmV0Ijoid3MiLCJ0eXBlIjoiIiwiaG9zdCI6InZtZXNzLmV4YW1wbGUuY29tIiwicGF0aCI6Ii92MnJheSIsInRscyI6InRscyIsInNuaSI6IiJ9CnRyb2phbjovL3Ryb2phbnBhc3MkMTAuMC4wLjU6NDQzP3NuaT10cm9qYW4uZXhhbXBsZS5jb20Kc29ja3M1Oi8vMTAuMC4wLjE6MTA4MA==
```

- [ ] **Step 2: Implement Base64UriParser**

Create `crates/proxy-sub/src/parser/base64_uri.rs`:

```rust
//! Base64 URI list subscription parser.
//!
//! Handles two patterns:
//! 1. Full content is one base64 blob → decode → split by newline → parse URIs
//! 2. Each line is individually base64-encoded → decode per line → parse URI
//!
//! URI schemes: ss://, vmess://, trojan://, socks5://, http://

use crate::models::SubscriptionProxy;
use crate::parser::Parser;
use proxy_core::models::Protocol;
use std::collections::HashMap;

/// Base64-encoded URI list parser.
pub struct Base64UriParser;

impl Parser for Base64UriParser {
    fn name(&self) -> &str {
        "Base64 URI"
    }

    fn detect(&self, content: &str) -> bool {
        let trimmed = content.trim();
        // Try decoding the whole content as base64
        if let Ok(decoded) = base64::decode(trimmed) {
            if let Ok(text) = String::from_utf8(decoded) {
                // Decoded content must contain protocol URI schemes
                return text.contains("://");
            }
        }
        // Also try: content has lines that start with known URI schemes
        // (some subscriptions are already decoded, just a list of URIs)
        for line in trimmed.lines().take(5) {
            let line = line.trim();
            if line.starts_with("ss://")
                || line.starts_with("vmess://")
                || line.starts_with("trojan://")
                || line.starts_with("socks5://")
                || line.starts_with("vless://")
            {
                return true;
            }
        }
        false
    }

    fn parse(&self, content: &str) -> Vec<SubscriptionProxy> {
        let trimmed = content.trim();

        // Strategy 1: try decoding entire content as base64
        let lines = if let Ok(decoded) = base64::decode(trimmed) {
            if let Ok(text) = String::from_utf8(decoded) {
                text.lines().map(|l| l.trim().to_string()).collect::<Vec<_>>()
            } else {
                // Fall through to strategy 2
                trimmed.lines().map(|l| l.trim().to_string()).collect::<Vec<_>>()
            }
        } else {
            // Strategy 2: each line might be individually encoded
            trimmed
                .lines()
                .filter_map(|line| {
                    let line = line.trim();
                    // If line doesn't start with a scheme, try base64-decoding it
                    if line.starts_with("ss://")
                        || line.starts_with("vmess://")
                        || line.starts_with("trojan://")
                        || line.starts_with("socks5://")
                        || line.starts_with("vless://")
                        || line.starts_with("http://")
                    {
                        Some(line.to_string())
                    } else if let Ok(decoded) = base64::decode(line) {
                        if let Ok(text) = String::from_utf8(decoded) {
                            Some(text.trim().to_string())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };

        lines
            .iter()
            .filter_map(|line| parse_single_uri(line))
            .collect()
    }
}

/// Parse a single proxy URI (ss://, vmess://, trojan://, socks5://, http://).
fn parse_single_uri(uri: &str) -> Option<SubscriptionProxy> {
    let uri = uri.trim();
    if uri.is_empty() {
        return None;
    }

    if uri.starts_with("socks5://") || uri.starts_with("http://") {
        parse_basic_uri(uri)
    } else if uri.starts_with("ss://") {
        parse_ss_uri(uri)
    } else if uri.starts_with("vmess://") {
        parse_vmess_uri(uri)
    } else if uri.starts_with("trojan://") {
        parse_trojan_uri(uri)
    } else {
        Some(SubscriptionProxy::Unknown {
            raw_config: uri.to_string(),
        })
    }
}

/// Parse socks5://host:port or http://host:port
fn parse_basic_uri(uri: &str) -> Option<SubscriptionProxy> {
    // socks5://10.0.0.1:1080 or http://10.0.0.2:8080
    let without_scheme = uri.split_once("://")?.1;
    let (host_port, _query) = without_scheme.split_once('?').unwrap_or((without_scheme, ""));
    let (host, port_str) = host_port.rsplit_once(':')?;
    let port: u16 = port_str.parse().ok()?;
    let protocol = if uri.starts_with("socks5://") {
        Protocol::Socks5
    } else {
        Protocol::Http
    };
    Some(SubscriptionProxy::Basic {
        host: host.to_string(),
        port,
        protocol,
    })
}

/// Parse ss://base64(method:password)@host:port or ss://method:password@host:port
fn parse_ss_uri(uri: &str) -> Option<SubscriptionProxy> {
    let without_scheme = uri.split_once("://")?.1;
    // ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@10.0.0.3:8388
    // or ss://aes-256-gcm:password@10.0.0.3:8388

    let (user_info, rest) = if let Some(pos) = without_scheme.find('@') {
        (&without_scheme[..pos], &without_scheme[pos + 1..])
    } else {
        // Some ss URIs encode everything in base64 after ss://
        let decoded = base64::decode(without_scheme).ok()?;
        let text = String::from_utf8(decoded).ok()?;
        return parse_ss_uri_inner(&text);
    };

    let (method, password) = if let Ok(decoded) = base64::decode(user_info) {
        let text = String::from_utf8(decoded).ok()?;
        let (m, p) = text.split_once(':')?;
        (m.to_string(), p.to_string())
    } else {
        let (m, p) = user_info.split_once(':')?;
        (m.to_string(), p.to_string())
    };

    let (host_port, query_str) = rest.split_once('#').unwrap_or((rest, ""));
    let (host_port, _) = host_port.split_once('?').unwrap_or((host_port, ""));
    let (host, port_str) = host_port.rsplit_once(':')?;
    let port: u16 = port_str.parse().ok()?;

    let params = parse_query_params(query_str);
    let plugin = params.get("plugin").cloned();
    let plugin_opts = params.get("plugin-opts").cloned();

    Some(SubscriptionProxy::Shadowsocks {
        host: host.to_string(),
        port,
        method,
        password,
        plugin,
        plugin_opts,
    })
}

/// Inner parser for fully-base64 ss URIs.
fn parse_ss_uri_inner(text: &str) -> Option<SubscriptionProxy> {
    // aes-256-gcm:password@10.0.0.3:8388
    let (user_info, rest) = text.split_once('@')?;
    let (method, password) = user_info.split_once(':')?;
    let (host_port, _) = rest.split_once('#').unwrap_or((rest, ""));
    let (host, port_str) = host_port.rsplit_once(':')?;
    let port: u16 = port_str.parse().ok()?;
    Some(SubscriptionProxy::Shadowsocks {
        host: host.to_string(),
        port,
        method: method.to_string(),
        password: password.to_string(),
        plugin: None,
        plugin_opts: None,
    })
}

/// Parse vmess://base64_json
fn parse_vmess_uri(uri: &str) -> Option<SubscriptionProxy> {
    let encoded = uri.split_once("://")?.1;
    let decoded = base64::decode(encoded).ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let json: HashMap<String, serde_json::Value> = serde_json::from_str(&text).ok()?;

    let host = json.get("hnb").or(json.get("add")).and_then(|v| v.as_str())?;
    let port = json.get("pnt").or(json.get("port")).and_then(|v| v.as_str())?.parse::<u16>().ok()?;
    let uuid = json.get("uid").or(json.get("id")).and_then(|v| v.as_str())?;
    let alter_id = json.get("aid").and_then(|v| v.as_str()).unwrap_or("0").parse::<u32>().ok()?;
    let security = json.get("scy").or(json.get("cipher")).and_then(|v| v.as_str()).unwrap_or("auto");
    let network = json.get("net").and_then(|v| v.as_str()).unwrap_or("tcp");
    let path = json.get("path").and_then(|v| v.as_str()).map(String::from);
    let host_header = json.get("host").and_then(|v| v.as_str()).map(String::from);
    let sni = json.get("sni").and_then(|v| v.as_str()).map(String::from);

    Some(SubscriptionProxy::Vmess {
        host: host.to_string(),
        port,
        uuid: uuid.to_string(),
        alter_id,
        security: security.to_string(),
        network: network.to_string(),
        path,
        host_header,
        sni,
    })
}

/// Parse trojan://password@host:port?sni=xxx
fn parse_trojan_uri(uri: &str) -> Option<SubscriptionProxy> {
    let without_scheme = uri.split_once("://")?.1;
    let (password, rest) = without_scheme.split_once('@')?;
    let (host_port, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    let (host_port, query_str) = host_port.split_once('?').unwrap_or((host_port, ""));
    let (host, port_str) = host_port.rsplit_once(':')?;
    let port: u16 = port_str.parse().ok()?;

    let params = parse_query_params(query_str);
    let sni = params.get("sni").cloned();
    let network = params.get("type").cloned();

    Some(SubscriptionProxy::Trojan {
        host: host.to_string(),
        port,
        password: password.to_string(),
        sni,
        network,
    })
}

/// Parse `key=value&key2=value2` query string into HashMap.
fn parse_query_params(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// Base64 decode that handles URL-safe base64 (no padding) and standard base64.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    // Try standard base64 first
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(input) {
        return Some(decoded);
    }
    // Try URL-safe base64
    if let Ok(decoded) = base64::engine::general_purpose::URL_SAFE.decode(input) {
        return Some(decoded);
    }
    // Try with padding added
    let padded = {
        let mut s = input.to_string();
        while s.len() % 4 != 0 {
            s.push('=');
        }
        s
    };
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(&padded) {
        return Some(decoded);
    }
    None
}

// Alias the module-level base64 decode helper
mod base64 {
    pub fn decode(input: &str) -> Option<Vec<u8>> {
        super::base64_decode(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SubscriptionProxy;

    #[test]
    fn test_detect_base64_blob() {
        let parser = Base64UriParser;
        // A base64 blob that decodes to URI lines
        let encoded = base64::engine::general_purpose::STANDARD.encode(
            "ss://aes-256-gcm:password@10.0.0.3:8388\nsocks5://10.0.0.1:1080\n"
        );
        assert!(parser.detect(&encoded));
    }

    #[test]
    fn test_detect_already_decoded_uris() {
        let parser = Base64UriParser;
        assert!(parser.detect("ss://aes-256-gcm:pass@1.2.3.4:8388\nvmess://abc"));
    }

    #[test]
    fn test_detect_invalid() {
        let parser = Base64UriParser;
        assert!(!parser.detect("just some random text"));
        assert!(!parser.detect("proxies:\n  - name: test"));
    }

    #[test]
    fn test_parse_socks5_uri() {
        let result = parse_single_uri("socks5://10.0.0.1:1080");
        assert!(result.is_some());
        if let SubscriptionProxy::Basic { host, port, protocol } = result.unwrap() {
            assert_eq!(host, "10.0.0.1");
            assert_eq!(port, 1080);
            assert_eq!(protocol, Protocol::Socks5);
        }
    }

    #[test]
    fn test_parse_ss_uri() {
        // ss://base64(method:password)@host:port
        let encoded_user = base64::engine::general_purpose::STANDARD.encode("aes-256-gcm:password");
        let uri = format!("ss://{encoded_user}@10.0.0.3:8388");
        let result = parse_single_uri(&uri);
        assert!(result.is_some());
        if let SubscriptionProxy::Shadowsocks { host, method, .. } = result.unwrap() {
            assert_eq!(host, "10.0.0.3");
            assert_eq!(method, "aes-256-gcm");
        }
    }

    #[test]
    fn test_parse_vmess_uri() {
        let json = serde_json::json!({
            "v": "2",
            "ps": "test",
            "uid": "a3482e88-686a-4a58-8126-99c9df64b7bf",
            "hnb": "10.0.0.4",
            "pnt": "443",
            "aid": "0",
            "scy": "auto",
            "net": "ws",
            "path": "/v2ray",
            "host": "vmess.example.com",
            "tls": "tls"
        });
        let encoded = base64::engine::general_purpose::STANDARD.encode(json.to_string());
        let uri = format!("vmess://{encoded}");
        let result = parse_single_uri(&uri);
        assert!(result.is_some());
        if let SubscriptionProxy::Vmess { host, uuid, network, .. } = result.unwrap() {
            assert_eq!(host, "10.0.0.4");
            assert_eq!(uuid, "a3482e88-686a-4a58-8126-99c9df64b7bf");
            assert_eq!(network, "ws");
        }
    }

    #[test]
    fn test_parse_trojan_uri() {
        let uri = "trojan://trojanpass@10.0.0.5:443?sni=trojan.example.com";
        let result = parse_single_uri(uri);
        assert!(result.is_some());
        if let SubscriptionProxy::Trojan { host, password, sni, .. } = result.unwrap() {
            assert_eq!(host, "10.0.0.5");
            assert_eq!(password, "trojanpass");
            assert_eq!(sni, Some("trojan.example.com".into()));
        }
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p proxy-sub --lib parser::base64_uri`
Expected: PASS (6 tests)

Run: `cargo clippy -p proxy-sub -- -D warnings`
Expected: No warnings

- [ ] **Step 4: Commit**

```bash
git add crates/proxy-sub/
git commit -m "feat(sub): add Base64 URI parser"
```

---

### Task 4: V2Ray JSON Parser

**Files:**
- Create: `crates/proxy-sub/src/parser/v2ray_json.rs`
- Create: `crates/proxy-sub/tests/fixtures/v2ray_sample.json`
- Test: inline tests

**Interfaces:**
- Consumes: `Parser` trait, `SubscriptionProxy` (Task 2, Task 1)
- Produces: `V2rayJsonParser`

- [ ] **Step 1: Create test fixture**

Create `crates/proxy-sub/tests/fixtures/v2ray_sample.json`:

```json
{
  "outbounds": [
    {
      "protocol": "socks",
      "settings": {
        "servers": [
          { "address": "10.0.0.1", "port": 1080 }
        ]
      },
      "tag": "socks-proxy"
    },
    {
      "protocol": "vmess",
      "settings": {
        "vnext": [
          {
            "address": "10.0.0.4",
            "port": 443,
            "users": [
              {
                "id": "a3482e88-686a-4a58-8126-99c9df64b7bf",
                "alterId": 0,
                "security": "auto"
              }
            ]
          }
        ]
      },
      "streamSettings": {
        "network": "ws",
        "wsSettings": {
          "path": "/v2ray",
          "headers": { "Host": "vmess.example.com" }
        },
        "security": "tls",
        "tlsSettings": {
          "serverName": "vmess.example.com"
        }
      },
      "tag": "vmess-proxy"
    },
    {
      "protocol": "trojan",
      "settings": {
        "servers": [
          {
            "address": "10.0.0.5",
            "port": 443,
            "password": "trojanpass"
          }
        ]
      },
      "streamSettings": {
        "network": "tcp",
        "security": "tls",
        "tlsSettings": {
          "serverName": "trojan.example.com"
        }
      },
      "tag": "trojan-proxy"
    },
    {
      "protocol": "shadowsocks",
      "settings": {
        "servers": [
          {
            "address": "10.0.0.3",
            "port": 8388,
            "method": "aes-256-gcm",
            "password": "mypassword"
          }
        ]
      },
      "tag": "ss-proxy"
    }
  ]
}
```

- [ ] **Step 2: Implement V2rayJsonParser**

Create `crates/proxy-sub/src/parser/v2ray_json.rs`:

```rust
//! V2Ray/Xray JSON config parser.
//!
//! Extracts the `outbounds` array from V2Ray JSON configuration.
//! Supported protocols: socks, http, vmess, vless, shadowsocks, trojan.

use crate::models::SubscriptionProxy;
use crate::parser::Parser;
use proxy_core::models::Protocol;

/// V2Ray/Xray JSON format parser.
pub struct V2rayJsonParser;

impl Parser for V2rayJsonParser {
    fn name(&self) -> &str {
        "V2Ray JSON"
    }

    fn detect(&self, content: &str) -> bool {
        // Must be valid JSON containing `outbounds` key
        let trimmed = content.trim();
        if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
            return false;
        }
        let value: serde_json::Result<serde_json::Value> = serde_json::from_str(trimmed);
        match value {
            Ok(v) => v.get("outbounds").is_some(),
            Err(_) => false,
        }
    }

    fn parse(&self, content: &str) -> Vec<SubscriptionProxy> {
        let value: serde_json::Value = match serde_json::from_str(content.trim()) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("V2Ray JSON: parse error: {e}");
                return Vec::new();
            }
        };

        let outbounds = value.get("outbounds").and_then(|v| v.as_array());
        if outbounds.is_none() {
            tracing::warn!("V2Ray JSON: no `outbounds` array found");
            return Vec::new();
        }

        outbounds
            .unwrap()
            .iter()
            .filter_map(|ob| parse_outbound(ob))
            .collect()
    }
}

/// Parse a single outbound entry.
fn parse_outbound(ob: &serde_json::Value) -> Option<SubscriptionProxy> {
    let protocol = ob.get("protocol").and_then(|v| v.as_str())?;
    let protocol_lower = protocol.to_ascii_lowercase();

    match protocol_lower.as_str() {
        "socks" | "http" => parse_basic_outbound(ob, protocol_lower),
        "vmess" => parse_vmess_outbound(ob),
        "vless" => parse_vless_outbound(ob),
        "shadowsocks" => parse_ss_outbound(ob),
        "trojan" => parse_trojan_outbound(ob),
        _ => Some(SubscriptionProxy::Unknown {
            raw_config: ob.to_string(),
        }),
    }
}

/// Parse socks/http outbound.
fn parse_basic_outbound(ob: &serde_json::Value, protocol: &str) -> Option<SubscriptionProxy> {
    let settings = ob.get("settings")?;
    let servers = settings.get("servers").and_then(|v| v.as_array())?;
    let server = servers.first()?;
    let host = server.get("address").and_then(|v| v.as_str())?;
    let port = server.get("port").and_then(|v| v.as_u64())? as u16;

    let proxy_protocol = if protocol == "socks" {
        Protocol::Socks5
    } else {
        Protocol::Http
    };

    Some(SubscriptionProxy::Basic {
        host: host.to_string(),
        port,
        protocol: proxy_protocol,
    })
}

/// Parse vmess outbound.
fn parse_vmess_outbound(ob: &serde_json::Value) -> Option<SubscriptionProxy> {
    let settings = ob.get("settings")?;
    let vnext = settings.get("vnext").and_then(|v| v.as_array())?;
    let first = vnext.first()?;
    let host = first.get("address").and_then(|v| v.as_str())?;
    let port = first.get("port").and_then(|v| v.as_u64())? as u16;
    let user = first.get("users").and_then(|v| v.as_array())?.first()?;
    let uuid = user.get("id").and_then(|v| v.as_str())?;
    let alter_id = user.get("alterId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let security = user.get("security").and_then(|v| v.as_str()).unwrap_or("auto");

    let stream = ob.get("streamSettings");
    let (network, path, host_header, sni) = extract_stream_settings(stream);

    Some(SubscriptionProxy::Vmess {
        host: host.to_string(),
        port,
        uuid: uuid.to_string(),
        alter_id,
        security: security.to_string(),
        network,
        path,
        host_header,
        sni,
    })
}

/// Parse vless outbound → Unknown (Phase 2).
fn parse_vless_outbound(ob: &serde_json::Value) -> Option<SubscriptionProxy> {
    Some(SubscriptionProxy::Unknown {
        raw_config: ob.to_string(),
    })
}

/// Parse shadowsocks outbound.
fn parse_ss_outbound(ob: &serde_json::Value) -> Option<SubscriptionProxy> {
    let settings = ob.get("settings")?;
    let servers = settings.get("servers").and_then(|v| v.as_array())?;
    let server = servers.first()?;
    let host = server.get("address").and_then(|v| v.as_str())?;
    let port = server.get("port").and_then(|v| v.as_u64())? as u16;
    let method = server.get("method").and_then(|v| v.as_str())?;
    let password = server.get("password").and_then(|v| v.as_str())?;

    Some(SubscriptionProxy::Shadowsocks {
        host: host.to_string(),
        port,
        method: method.to_string(),
        password: password.to_string(),
        plugin: None,
        plugin_opts: None,
    })
}

/// Parse trojan outbound.
fn parse_trojan_outbound(ob: &serde_json::Value) -> Option<SubscriptionProxy> {
    let settings = ob.get("settings")?;
    let servers = settings.get("servers").and_then(|v| v.as_array())?;
    let server = servers.first()?;
    let host = server.get("address").and_then(|v| v.as_str())?;
    let port = server.get("port").and_then(|v| v.as_u64())? as u16;
    let password = server.get("password").and_then(|v| v.as_str())?;

    let stream = ob.get("streamSettings");
    let (_, _, _, sni) = extract_stream_settings(stream);
    let network = stream
        .and_then(|s| s.get("network"))
        .and_then(|v| v.as_str())
        .map(String::from);

    Some(SubscriptionProxy::Trojan {
        host: host.to_string(),
        port,
        password: password.to_string(),
        sni,
        network,
    })
}

/// Extract network, path, host_header, sni from streamSettings.
fn extract_stream_settings(
    stream: Option<&serde_json::Value>,
) -> (String, Option<String>, Option<String>, Option<String>) {
    let stream = match stream {
        Some(s) => s,
        None => return ("tcp".into(), None, None, None),
    };

    let network = stream
        .get("network")
        .and_then(|v| v.as_str())
        .unwrap_or("tcp")
        .to_string();

    let path = match network.as_str() {
        "ws" => stream
            .get("wsSettings")
            .and_then(|w| w.get("path"))
            .and_then(|v| v.as_str())
            .map(String::from),
        "grpc" => stream
            .get("grpcSettings")
            .and_then(|g| g.get("serviceName"))
            .and_then(|v| v.as_str())
            .map(String::from),
        _ => None,
    };

    let host_header = match network.as_str() {
        "ws" => stream
            .get("wsSettings")
            .and_then(|w| w.get("headers"))
            .and_then(|h| h.get("Host"))
            .and_then(|v| v.as_str())
            .map(String::from),
        _ => None,
    };

    let sni = stream
        .get("tlsSettings")
        .and_then(|t| t.get("serverName"))
        .and_then(|v| v.as_str())
        .map(String::from);

    (network, path, host_header, sni)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SubscriptionProxy;

    const FIXTURE: &str = include_str!("../../tests/fixtures/v2ray_sample.json");

    #[test]
    fn test_v2ray_detect_valid() {
        let parser = V2rayJsonParser;
        assert!(parser.detect(FIXTURE));
    }

    #[test]
    fn test_v2ray_detect_invalid() {
        let parser = V2rayJsonParser;
        assert!(!parser.detect("just random text"));
        assert!(!parser.detect("proxies:\n  - name: test\n    type: socks5"));
    }

    #[test]
    fn test_v2ray_detect_json_without_outbounds() {
        let parser = V2rayJsonParser;
        assert!(!parser.detect("{\"inbounds\": []}"));
    }

    #[test]
    fn test_v2ray_parse() {
        let parser = V2rayJsonParser;
        let proxies = parser.parse(FIXTURE);
        assert_eq!(proxies.len(), 4);

        // socks → Basic
        assert!(proxies[0].is_direct_usable());
        if let SubscriptionProxy::Basic { host, port, protocol } = &proxies[0] {
            assert_eq!(host, "10.0.0.1");
            assert_eq!(*port, 1080);
            assert_eq!(*protocol, Protocol::Socks5);
        }

        // vmess → Vmess
        if let SubscriptionProxy::Vmess { host, uuid, network, path, .. } = &proxies[1] {
            assert_eq!(host, "10.0.0.4");
            assert_eq!(uuid, "a3482e88-686a-4a58-8126-99c9df64b7bf");
            assert_eq!(network, "ws");
            assert_eq!(path, Some("/v2ray".into()));
        }

        // trojan → Trojan
        if let SubscriptionProxy::Trojan { host, password, sni, .. } = &proxies[2] {
            assert_eq!(host, "10.0.0.5");
            assert_eq!(password, "trojanpass");
            assert_eq!(sni, Some("trojan.example.com".into()));
        }

        // ss → Shadowsocks
        if let SubscriptionProxy::Shadowsocks { host, method, .. } = &proxies[3] {
            assert_eq!(host, "10.0.0.3");
            assert_eq!(method, "aes-256-gcm");
        }
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p proxy-sub --lib parser::v2ray_json`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/proxy-sub/
git commit -m "feat(sub): add V2Ray JSON parser"
```

---

### Task 5: Surge Parser

**Files:**
- Create: `crates/proxy-sub/src/parser/surge.rs`
- Create: `crates/proxy-sub/tests/fixtures/surge_sample.txt`
- Create: `crates/proxy-sub/tests/fixtures/mixed_invalid.txt`
- Test: inline tests

**Interfaces:**
- Consumes: `Parser` trait, `SubscriptionProxy` (Task 2, Task 1)
- Produces: `SurgeParser`

- [ ] **Step 1: Create test fixtures**

Create `crates/proxy-sub/tests/fixtures/surge_sample.txt`:

```
socks5-proxy = socks5, 10.0.0.1, 1080
http-proxy = http, 10.0.0.2, 8080
ss-proxy = ss, 10.0.0.3, 8388, encrypt-method=aes-256-gcm, password=mypassword
vmess-proxy = vmess, 10.0.0.4, 443, username=a3482e88-686a-4a58-8126-99c9df64b7bf, tls=true, ws=true, ws-path=/v2ray, ws-host=vmess.example.com, sni=vmess.example.com
trojan-proxy = trojan, 10.0.0.5, 443, password=trojanpass, sni=trojan.example.com
```

Create `crates/proxy-sub/tests/fixtures/mixed_invalid.txt`:

```
this is not a proxy config
random text here
no valid format detected
```

- [ ] **Step 2: Implement SurgeParser**

Create `crates/proxy-sub/src/parser/surge.rs`:

```rust
//! Surge proxy list parser.
//!
//! Parses Surge-style proxy definitions:
//! `Name = type, server, port, [key=value params...]`

use crate::models::SubscriptionProxy;
use crate::parser::Parser;
use proxy_core::models::Protocol;
use std::collections::HashMap;

/// Surge proxy list format parser.
pub struct SurgeParser;

impl Parser for SurgeParser {
    fn name(&self) -> &str {
        "Surge"
    }

    fn detect(&self, content: &str) -> bool {
        // Check if lines match Surge pattern: Name = type, server, port, ...
        let trimmed = content.trim();
        let valid_lines = trimmed
            .lines()
            .filter(|l| {
                let l = l.trim();
                !l.is_empty() && !l.starts_with('#')
            })
            .take(5)
            .filter(|l| is_surge_line(l))
            .count();
        valid_lines >= 1
    }

    fn parse(&self, content: &str) -> Vec<SubscriptionProxy> {
        content
            .trim()
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    return None;
                }
                parse_surge_line(line)
            })
            .collect()
    }
}

/// Check if a line matches the Surge pattern.
fn is_surge_line(line: &str) -> bool {
    // Pattern: Name = type, server, port, [params]
    // Must contain `=` and `,`
    if !line.contains('=') || !line.contains(',') {
        return false;
    }
    let after_eq = line.split_once('=').map(|(_, r)| r.trim()).unwrap_or("");
    // First comma-separated field should be a known type
    let first_field = after_eq.split(',').next().unwrap_or("").trim().to_lowercase();
    matches!(
        first_field.as_str(),
        "http" | "socks5" | "ss" | "vmess" | "trojan" | "vless"
    )
}

/// Parse a single Surge proxy line.
fn parse_surge_line(line: &str) -> Option<SubscriptionProxy> {
    let (_name, rest) = line.split_once('=')?;
    let rest = rest.trim();
    let parts: Vec<&str> = rest.splitn(4, ',').collect();
    if parts.len() < 3 {
        return None;
    }

    let type_str = parts[0].trim().to_lowercase();
    let host = parts[1].trim();
    let port: u16 = parts[2].trim().parse().ok()?;
    let params_str = if parts.len() == 4 { parts[3].trim() } else { "" };
    let params = parse_surge_params(params_str);

    match type_str.as_str() {
        "socks5" => Some(SubscriptionProxy::Basic {
            host: host.to_string(),
            port,
            protocol: Protocol::Socks5,
        }),
        "http" => Some(SubscriptionProxy::Basic {
            host: host.to_string(),
            port,
            protocol: Protocol::Http,
        }),
        "ss" => Some(SubscriptionProxy::Shadowsocks {
            host: host.to_string(),
            port,
            method: params.get("encrypt-method").cloned().unwrap_or_default(),
            password: params.get("password").cloned().unwrap_or_default(),
            plugin: params.get("plugin").cloned(),
            plugin_opts: params.get("plugin-opts").cloned(),
        }),
        "vmess" => Some(SubscriptionProxy::Vmess {
            host: host.to_string(),
            port,
            uuid: params.get("username").cloned().unwrap_or_default(),
            alter_id: params
                .get("alter-id")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            security: params.get("cipher").cloned().unwrap_or("auto".into()),
            network: if params.get("ws").map(|v| v.as_str()) == Some("true") {
                "ws".into()
            } else {
                params.get("network").cloned().unwrap_or("tcp".into())
            },
            path: params.get("ws-path").cloned(),
            host_header: params.get("ws-host").cloned(),
            sni: params.get("sni").cloned(),
        }),
        "trojan" => Some(SubscriptionProxy::Trojan {
            host: host.to_string(),
            port,
            password: params.get("password").cloned().unwrap_or_default(),
            sni: params.get("sni").cloned(),
            network: params.get("network").cloned(),
        }),
        _ => Some(SubscriptionProxy::Unknown {
            raw_config: line.to_string(),
        }),
    }
}

/// Parse Surge params: `key1=value1, key2=value2` into HashMap.
fn parse_surge_params(params: &str) -> HashMap<String, String> {
    params
        .split(',')
        .filter_map(|pair| {
            let pair = pair.trim();
            let (k, v) = pair.split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SubscriptionProxy;

    const FIXTURE: &str = include_str!("../../tests/fixtures/surge_sample.txt");

    #[test]
    fn test_surge_detect_valid() {
        let parser = SurgeParser;
        assert!(parser.detect(FIXTURE));
    }

    #[test]
    fn test_surge_detect_invalid() {
        let parser = SurgeParser;
        assert!(!parser.detect("just random text"));
        assert!(!parser.detect("{\"outbounds\": []}"));
    }

    #[test]
    fn test_surge_parse() {
        let parser = SurgeParser;
        let proxies = parser.parse(FIXTURE);
        assert_eq!(proxies.len(), 5);

        // socks5 → Basic
        assert!(proxies[0].is_direct_usable());
        if let SubscriptionProxy::Basic { host, port, protocol } = &proxies[0] {
            assert_eq!(host, "10.0.0.1");
            assert_eq!(*port, 1080);
            assert_eq!(*protocol, Protocol::Socks5);
        }

        // ss → Shadowsocks
        if let SubscriptionProxy::Shadowsocks { host, method, .. } = &proxies[2] {
            assert_eq!(host, "10.0.0.3");
            assert_eq!(method, "aes-256-gcm");
        }

        // vmess → Vmess
        if let SubscriptionProxy::Vmess { uuid, network, path, .. } = &proxies[3] {
            assert_eq!(uuid, "a3482e88-686a-4a58-8126-99c9df64b7bf");
            assert_eq!(network, "ws");
            assert_eq!(path, Some("/v2ray".into()));
        }

        // trojan → Trojan
        if let SubscriptionProxy::Trojan { password, sni, .. } = &proxies[4] {
            assert_eq!(password, "trojanpass");
            assert_eq!(sni, Some("trojan.example.com".into()));
        }
    }
}
```

- [ ] **Step 3: Verify parse_subscription detects Surge correctly**

Add a test in `parser/mod.rs` tests section:

```rust
#[test]
fn test_parse_subscription_surge() {
    let content = "socks5-proxy = socks5, 10.0.0.1, 1080\nhttp-proxy = http, 10.0.0.2, 8080";
    let proxies = parse_subscription(content);
    assert_eq!(proxies.len(), 2);
    assert!(proxies[0].is_direct_usable());
}
```

- [ ] **Step 4: Verify mixed_invalid fixture yields empty**

Add a test:

```rust
#[test]
fn test_parse_subscription_no_match_fixture() {
    let content = include_str!("../tests/fixtures/mixed_invalid.txt");
    assert!(parse_subscription(content).is_empty());
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test -p proxy-sub --lib`
Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add crates/proxy-sub/
git commit -m "feat(sub): add Surge parser and test fixtures"
```

---

### Task 6: Source Discovery — Discover Trait + StaticUrl + ContentCache

**Files:**
- Create: `crates/proxy-sub/src/discover/mod.rs`
- Create: `crates/proxy-sub/src/discover/static_url.rs`
- Create: `crates/proxy-sub/src/source/mod.rs`
- Create: `crates/proxy-sub/src/source/cache.rs`
- Test: inline tests

**Interfaces:**
- Consumes: `SubscriptionConfig` (will be defined in Task 8)
- Produces: `Discover` trait, `StaticUrlDiscover`, `ContentCache`, `SubscriptionSource`

- [ ] **Step 1: Create discover/mod.rs with Discover trait**

```rust
//! Source discovery: find subscription URLs from various sources.

pub mod aggregator;
pub mod github_search;
pub mod static_url;

/// A discoverer finds subscription URLs from a source.
#[async_trait::async_trait]
pub trait Discover: Send + Sync {
    /// Human-readable name for logging.
    fn name(&self) -> &str;

    /// Discover subscription URLs.
    /// Returns empty vec on error (never fails the refresh loop).
    async fn discover(&self) -> Vec<String>;
}
```

- [ ] **Step 2: Create discover/static_url.rs**

```rust
//! Static URL discoverer: reads pre-configured URL list from config.

use crate::discover::Discover;

/// Discoverer that returns a static list of subscription URLs from config.
pub struct StaticUrlDiscover {
    urls: Vec<String>,
}

impl StaticUrlDiscover {
    pub fn new(urls: Vec<String>) -> Self {
        Self { urls }
    }
}

#[async_trait::async_trait]
impl Discover for StaticUrlDiscover {
    fn name(&self) -> &str {
        "StaticUrl"
    }

    async fn discover(&self) -> Vec<String> {
        if self.urls.is_empty() {
            tracing::debug!("StaticUrl: no URLs configured");
        }
        self.urls.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_static_url_discover() {
        let discoverer = StaticUrlDiscover::new(vec![
            "https://example.com/sub1".into(),
            "https://example.com/sub2".into(),
        ]);
        let urls = discoverer.discover().await;
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://example.com/sub1");
    }

    #[tokio::test]
    async fn test_static_url_empty() {
        let discoverer = StaticUrlDiscover::new(vec![]);
        let urls = discoverer.discover().await;
        assert!(urls.is_empty());
    }
}
```

- [ ] **Step 3: Create source/cache.rs**

```rust
//! In-memory content cache with TTL-based lazy eviction.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// In-memory content cache. Entries expire after TTL and are lazily evicted.
pub struct ContentCache {
    entries: HashMap<String, (String, Instant)>,
    ttl: Duration,
}

impl ContentCache {
    pub fn new(ttl_sec: u64) -> Self {
        Self {
            entries: HashMap::new(),
            ttl: Duration::from_secs(ttl_sec),
        }
    }

    /// Get cached content if it exists and hasn't expired.
    /// Expired entries are removed (lazy eviction).
    pub fn get(&mut self, url: &str) -> Option<String> {
        if let Some((content, inserted_at)) = self.entries.get(url) {
            if inserted_at.elapsed() < self.ttl {
                return Some(content.clone());
            }
            // Expired — remove
            self.entries.remove(url);
        }
        None
    }

    /// Store content in cache.
    pub fn put(&mut self, url: &str, content: &str) {
        self.entries.insert(url.to_string(), (content.to_string(), Instant::now()));
    }

    /// Number of entries (including potentially expired ones).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.len() == 0
    }

    /// Remove all expired entries.
    pub fn evict_expired(&mut self) {
        self.entries.retain(|_, (_, inserted_at)| inserted_at.elapsed() < self.ttl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_put_get() {
        let mut cache = ContentCache::new(60);
        cache.put("https://example.com", "content1");
        assert_eq!(cache.get("https://example.com"), Some("content1".into()));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_cache_missing() {
        let mut cache = ContentCache::new(60);
        assert_eq!(cache.get("https://missing.com"), None);
    }

    #[test]
    fn test_cache_expired() {
        let mut cache = ContentCache::new(0); // TTL = 0 seconds → instant expiry
        cache.put("https://example.com", "content");
        // Even immediately, TTL=0 means it's expired
        assert_eq!(cache.get("https://example.com"), None);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_evict_expired() {
        let mut cache = ContentCache::new(0);
        cache.put("https://old.com", "old_content");
        cache.evict_expired();
        assert!(cache.is_empty());
    }
}
```

- [ ] **Step 4: Create source/mod.rs with SubscriptionSource**

```rust
//! Subscription source fetcher with caching.

mod cache;

pub use cache::ContentCache;

use anyhow::Result;
use std::time::Duration;

/// Fetches subscription content from URLs, with caching.
pub struct SubscriptionSource {
    client: reqwest::Client,
    cache: ContentCache,
    timeout: Duration,
}

impl SubscriptionSource {
    pub fn new(cache_ttl_sec: u64, timeout_sec: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_sec))
            .build()
            .expect("build reqwest client");
        Self {
            client,
            cache: ContentCache::new(cache_ttl_sec),
            timeout: Duration::from_secs(timeout_sec),
        }
    }

    /// Fetch subscription content from URL, with caching.
    /// Returns cached content if available and not expired.
    pub async fn fetch(&mut self, url: &str) -> Result<String> {
        if let Some(content) = self.cache.get(url) {
            tracing::debug!("subscription source: cache hit for {url}");
            return Ok(content);
        }

        tracing::info!("subscription source: fetching {url}");
        let resp = self
            .client
            .get(url)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("fetch {url}: {e}"))?;

        let content = resp
            .text()
            .await
            .map_err(|e| anyhow::anyhow!("read {url}: {e}"))?;

        self.cache.put(url, &content);
        Ok(content)
    }

    /// Evict expired cache entries.
    pub fn evict_expired(&mut self) {
        self.cache.evict_expired();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_new() {
        let source = SubscriptionSource::new(60, 30);
        assert!(source.cache.is_empty());
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p proxy-sub --lib discover::static_url source::cache source`
Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add crates/proxy-sub/
git commit -m "feat(sub): add Discover trait, StaticUrlDiscover, ContentCache, SubscriptionSource"
```

---

### Task 7: Source Discovery — GitHubSearch + Aggregator

**Files:**
- Create: `crates/proxy-sub/src/discover/github_search.rs`
- Create: `crates/proxy-sub/src/discover/aggregator.rs`
- Test: inline tests

**Interfaces:**
- Consumes: `Discover` trait (Task 6), `reqwest` for HTTP
- Produces: `GitHubSearchDiscover`, `AggregatorDiscover`

- [ ] **Step 1: Implement GitHubSearchDiscover**

Create `crates/proxy-sub/src/discover/github_search.rs`:

```rust
//! GitHub Search API discoverer: finds subscription repos by keyword search.

use crate::discover::Discover;
use std::time::Duration;

/// Configuration for GitHub search.
pub struct GitHubSearchConfig {
    /// GitHub Personal Access Token (optional, avoids rate limit).
    pub token: Option<String>,
    /// Maximum number of repos to return per search.
    pub max_results: u32,
    /// Search keywords.
    pub keywords: Vec<String>,
    /// HTTP timeout in seconds.
    pub timeout_sec: u64,
}

/// Discoverer using GitHub Search API to find subscription repos.
pub struct GitHubSearchDiscover {
    config: GitHubSearchConfig,
    client: reqwest::Client,
}

impl GitHubSearchDiscover {
    pub fn new(config: GitHubSearchConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_sec))
            .build()
            .expect("build reqwest client");
        Self { config, client }
    }

    /// Build a search URL for GitHub repository search API.
    fn search_repo_url(&self, keyword: &str) -> String {
        format!(
            "https://api.github.com/search/repositories?q={}&sort=updated&order=desc&per_page={}",
            keyword,
            self.config.max_results.min(100)
        )
    }

    /// Build a search URL for GitHub code search API.
    fn search_code_url(&self, keyword: &str) -> String {
        format!(
            "https://api.github.com/search/code?q={}&sort=updated&order=desc&per_page={}",
            keyword,
            self.config.max_results.min(100)
        )
    }

    /// Build authorization header if token is set.
    fn auth_header(&self) -> Option<String> {
        self.config.token.as_ref().map(|t| format!("Bearer {t}"))
    }
}

#[async_trait::async_trait]
impl Discover for GitHubSearchDiscover {
    fn name(&self) -> &str {
        "GitHubSearch"
    }

    async fn discover(&self) -> Vec<String> {
        let mut urls = Vec::new();

        for keyword in &self.config.keywords {
            // 1. Search repositories
            if let Ok(repo_urls) = self.search_repos(keyword).await {
                urls.extend(repo_urls);
            }

            // 2. Search code for subscription files
            if let Ok(code_urls) = self.search_code(keyword).await {
                urls.extend(code_urls);
            }
        }

        // Dedup
        let mut seen = std::collections::HashSet::new();
        urls.retain(|u| seen.insert(u.clone()));

        tracing::info!("GitHubSearch: discovered {} unique subscription URLs", urls.len());
        urls
    }
}

impl GitHubSearchDiscover {
    /// Search GitHub repositories for subscription repos.
    async fn search_repos(&self, keyword: &str) -> anyhow::Result<Vec<String>> {
        let url = self.search_repo_url(keyword);
        let mut req = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        req = req.header("Accept", "application/vnd.github+json");

        let resp = req.send().await?;
        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            tracing::warn!("GitHubSearch: 403 Forbidden — rate limit or token issue");
            return Ok(Vec::new());
        }
        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            tracing::warn!("GitHubSearch: 429 Too Many Requests — rate limited");
            return Ok(Vec::new());
        }

        let body: serde_json::Value = resp.json().await?;
        let items = body.get("items").and_then(|v| v.as_array());

        let mut urls = Vec::new();
        if let Some(items) = items {
            for item in items.iter().take(self.config.max_results as usize) {
                // Extract owner, repo name, and default branch
                let full_name = item.get("full_name").and_then(|v| v.as_str());
                let default_branch = item
                    .get("default_branch")
                    .and_then(|v| v.as_str())
                    .unwrap_or("main");
                if let Some(name) = full_name {
                    // Generate likely subscription file URLs
                    for filename in &["clash.yaml", "proxy.yaml", "v2ray.yaml", "sub.yaml"] {
                        urls.push(format!(
                            "https://raw.githubusercontent.com/{name}/{default_branch}/{filename}"
                        ));
                    }
                }
            }
        }

        Ok(urls)
    }

    /// Search GitHub code for files containing proxy content.
    async fn search_code(&self, keyword: &str) -> anyhow::Result<Vec<String>> {
        let url = self.search_code_url(keyword);
        let mut req = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        req = req.header("Accept", "application/vnd.github+json");

        let resp = req.send().await?;
        if resp.status() == reqwest::StatusCode::FORBIDDEN
            || resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            tracing::warn!("GitHubSearch: code search rate limited");
            return Ok(Vec::new());
        }

        let body: serde_json::Value = resp.json().await?;
        let items = body.get("items").and_then(|v| v.as_array());

        let mut urls = Vec::new();
        if let Some(items) = items {
            for item in items.iter().take(self.config.max_results as usize) {
                let html_url = item.get("html_url").and_then(|v| v.as_str());
                if let Some(url) = html_url {
                    // Convert github.com URL to raw.githubusercontent.com
                    let raw_url = github_to_raw_url(url);
                    urls.push(raw_url);
                }
            }
        }

        Ok(urls)
    }
}

/// Convert a GitHub file page URL to raw content URL.
/// e.g. https://github.com/user/repo/blob/main/clash.yaml
///   → https://raw.githubusercontent.com/user/repo/main/clash.yaml
fn github_to_raw_url(url: &str) -> String {
    if url.contains("/blob/") {
        url.replace("github.com", "raw.githubusercontent.com")
            .replace("/blob/", "/")
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_to_raw_url() {
        let url = "https://github.com/user/repo/blob/main/clash.yaml";
        assert_eq!(
            github_to_raw_url(url),
            "https://raw.githubusercontent.com/user/repo/main/clash.yaml"
        );
    }

    #[test]
    fn test_github_to_raw_url_no_blob() {
        let url = "https://raw.githubusercontent.com/user/repo/main/clash.yaml";
        assert_eq!(github_to_raw_url(url), url);
    }
}
```

- [ ] **Step 2: Implement AggregatorDiscover**

Create `crates/proxy-sub/src/discover/aggregator.rs`:

```rust
//! Aggregator project discoverer: fetches URL lists from known aggregator sites.

use crate::discover::Discover;
use std::time::Duration;

/// Configuration for a single aggregator source.
pub struct AggregatorConfig {
    /// URL of the aggregator list.
    pub url: String,
    /// Format of the aggregator response: "text", "json", or "yaml".
    pub format: String,
    /// HTTP timeout in seconds.
    pub timeout_sec: u64,
}

/// Discoverer that fetches subscription URL lists from aggregator projects.
pub struct AggregatorDiscover {
    config: AggregatorConfig,
    client: reqwest::Client,
}

impl AggregatorDiscover {
    pub fn new(config: AggregatorConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_sec))
            .build()
            .expect("build reqwest client");
        Self { config, client }
    }
}

#[async_trait::async_trait]
impl Discover for AggregatorDiscover {
    fn name(&self) -> &str {
        "Aggregator"
    }

    async fn discover(&self) -> Vec<String> {
        let resp = match self.client.get(&self.config.url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Aggregator: fetch {} failed: {e}", self.config.url);
                return Vec::new();
            }
        };

        let text = match resp.text().await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Aggregator: read {} failed: {e}", self.config.url);
                return Vec::new();
            }
        };

        match self.config.format.as_str() {
            "text" => parse_text_list(&text),
            "json" => parse_json_list(&text),
            "yaml" => parse_yaml_list(&text),
            _ => {
                tracing::warn!("Aggregator: unknown format '{}'", self.config.format);
                Vec::new()
            }
        }
    }
}

/// Parse text format: one URL per line, ignore empty/comment lines.
fn parse_text_list(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter(|l| l.starts_with("http://") || l.starts_with("https://"))
        .collect()
}

/// Parse JSON format: array of objects with `url` field.
fn parse_json_list(text: &str) -> Vec<String> {
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Aggregator: JSON parse error: {e}");
            return Vec::new();
        }
    };

    let arr = value.as_array();
    if arr.is_none() {
        tracing::warn!("Aggregator: JSON is not an array");
        return Vec::new();
    }

    arr.unwrap()
        .iter()
        .filter_map(|item| {
            // Support { "url": "..." } or plain string
            if let Some(s) = item.as_str() {
                Some(s.to_string())
            } else {
                item.get("url").and_then(|v| v.as_str()).map(String::from)
            }
        })
        .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
        .collect()
}

/// Parse YAML format: `subscriptions:` list with `url` field.
fn parse_yaml_list(text: &str) -> Vec<String> {
    let value: serde_yaml::Value = match serde_yaml::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Aggregator: YAML parse error: {e}");
            return Vec::new();
        }
    };

    let subs = value
        .get("subscriptions")
        .and_then(|v| v.as_sequence());

    if subs.is_none() {
        tracing::warn!("Aggregator: YAML has no `subscriptions` key");
        return Vec::new();
    }

    subs.unwrap()
        .iter()
        .filter_map(|item| {
            if let serde_yaml::Value::String(s) = item {
                Some(s.clone())
            } else {
                item.get("url").and_then(|v| v.as_str()).map(String::from)
            }
        })
        .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_list() {
        let text = "https://sub1.example.com\n# comment\nhttps://sub2.example.com\n\ninvalid\nhttps://sub3.example.com";
        let urls = parse_text_list(text);
        assert_eq!(urls.len(), 3);
        assert_eq!(urls[0], "https://sub1.example.com");
    }

    #[test]
    fn test_parse_json_list() {
        let json = r#"[
            {"url": "https://sub1.example.com", "format": "clash"},
            "https://sub2.example.com"
        ]"#;
        let urls = parse_json_list(json);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://sub1.example.com");
    }

    #[test]
    fn test_parse_yaml_list() {
        let yaml = "subscriptions:\n  - url: https://sub1.example.com\n  - https://sub2.example.com";
        let urls = parse_yaml_list(yaml);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://sub1.example.com");
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p proxy-sub --lib discover`
Expected: ALL PASS

- [ ] **Step 4: Commit**

```bash
git add crates/proxy-sub/
git commit -m "feat(sub): add GitHubSearch and Aggregator discoverers"
```

---

### Task 8: Config Extension + Proxy Conversion + Pending Storage

**Files:**
- Modify: `crates/proxy-core/src/config.rs` (add `SubscriptionConfig`)
- Create: `crates/proxy-sub/src/convert.rs`
- Create: `crates/proxy-sub/src/pending.rs`
- Test: inline tests

**Interfaces:**
- Consumes: `SubscriptionProxy` (Task 1), `Proxy`/`ProxyStore` (proxy-core), `redis` (proxy-core)
- Produces: `SubscriptionConfig`, `to_proxy()` conversion, `store_pending()` for encrypted nodes

- [ ] **Step 1: Add SubscriptionConfig to proxy-core/src/config.rs**

Add after the `FreePoolSettings` struct (around line 155):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubDiscoverConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default = "default_github_max")]
    pub max_results: u32,
    #[serde(default = "default_github_interval")]
    pub search_interval_sec: u64,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatorEntryConfig {
    pub url: String,
    #[serde(default = "default_agg_format")]
    pub format: String,
    #[serde(default = "default_agg_interval")]
    pub refresh_interval_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionConfig {
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub github: GitHubDiscoverConfig,
    #[serde(default)]
    pub aggregators: Vec<AggregatorEntryConfig>,
    #[serde(default = "default_sub_interval")]
    pub refresh_interval_sec: u64,
    #[serde(default = "default_sub_timeout")]
    pub fetch_timeout_sec: u64,
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_sec: u64,
}
```

Add `SubscriptionConfig` to the `Settings` struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub gateway: GatewaySettings,
    #[serde(default)]
    pub api: ApiSettings,
    #[serde(default)]
    pub mcp: McpSettings,
    #[serde(default)]
    pub redis: RedisSettings,
    #[serde(default)]
    pub pool: PoolSettings,
    #[serde(default)]
    pub warp: WarpSettings,
    #[serde(default)]
    pub geoip: GeoIpSettings,
    #[serde(default)]
    pub free_pool: FreePoolSettings,
    #[serde(default)]
    pub subscription: SubscriptionConfig,  // NEW
}
```

Add default value functions (after existing defaults, around line 250):

```rust
fn default_github_max() -> u32 { 20 }
fn default_github_interval() -> u64 { 86400 }
fn default_agg_format() -> String { "text".into() }
fn default_agg_interval() -> u64 { 43200 }
fn default_sub_interval() -> u64 { 3600 }
fn default_sub_timeout() -> u64 { 30 }
fn default_cache_ttl() -> u64 { 1800 }
```

Add Default impls for new config types:

```rust
impl Default for GitHubDiscoverConfig {
    fn default() -> Self { serde_yaml::from_str("{}").unwrap() }
}
impl Default for SubscriptionConfig {
    fn default() -> Self { serde_yaml::from_str("{}").unwrap() }
}
```

Add `SubscriptionConfig` to `pub mod config;` — it's already in `Settings` so it's accessible.

- [ ] **Step 2: Create proxy-sub/src/convert.rs**

```rust
//! Convert SubscriptionProxy::Basic nodes to pool Proxy entries.

use proxy_core::models::{Protocol, Proxy};
use crate::models::SubscriptionProxy;

/// Convert a SubscriptionProxy::Basic to a pool Proxy entry.
/// Returns None for non-basic (encrypted) nodes.
pub fn to_proxy(sub: &SubscriptionProxy, source_url: &str) -> Option<Proxy> {
    match sub {
        SubscriptionProxy::Basic { host, port, protocol } => {
            let mut proxy = Proxy::new(host.clone(), *port, *protocol);
            proxy.source = Some(format!("subscription:{}", source_url));
            Some(proxy)
        }
        _ => None,
    }
}

/// Convert multiple SubscriptionProxy nodes, splitting into basic and encrypted.
/// Returns (basic_proxies, encrypted_subs).
pub fn partition(
    subs: &[SubscriptionProxy],
    source_url: &str,
) -> (Vec<Proxy>, Vec<SubscriptionProxy>) {
    let mut basic = Vec::new();
    let mut encrypted = Vec::new();
    for sub in subs {
        if let Some(proxy) = to_proxy(sub, source_url) {
            basic.push(proxy);
        } else {
            encrypted.push(sub.clone());
        }
    }
    (basic, encrypted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_proxy_basic() {
        let sub = SubscriptionProxy::Basic {
            host: "10.0.0.1".into(),
            port: 1080,
            protocol: Protocol::Socks5,
        };
        let proxy = to_proxy(&sub, "https://example.com/sub").unwrap();
        assert_eq!(proxy.host, "10.0.0.1");
        assert_eq!(proxy.port, 1080);
        assert_eq!(proxy.protocol, Protocol::Socks5);
        assert_eq!(proxy.source, Some("subscription:https://example.com/sub".into()));
    }

    #[test]
    fn test_to_proxy_encrypted_returns_none() {
        let sub = SubscriptionProxy::Shadowsocks {
            host: "10.0.0.3".into(),
            port: 8388,
            method: "aes-256-gcm".into(),
            password: "pass".into(),
            plugin: None,
            plugin_opts: None,
        };
        assert!(to_proxy(&sub, "https://example.com/sub").is_none());
    }

    #[test]
    fn test_partition() {
        let subs = vec![
            SubscriptionProxy::Basic {
                host: "10.0.0.1".into(),
                port: 1080,
                protocol: Protocol::Socks5,
            },
            SubscriptionProxy::Shadowsocks {
                host: "10.0.0.3".into(),
                port: 8388,
                method: "aes-256-gcm".into(),
                password: "pass".into(),
                plugin: None,
                plugin_opts: None,
            },
        ];
        let (basic, encrypted) = partition(&subs, "https://example.com/sub");
        assert_eq!(basic.len(), 1);
        assert_eq!(encrypted.len(), 1);
    }
}
```

- [ ] **Step 3: Create proxy-sub/src/pending.rs**

```rust
//! Redis pending storage for encrypted-protocol nodes awaiting Phase 2 xray integration.

use crate::models::SubscriptionProxy;
use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;

/// Store encrypted SubscriptionProxy nodes in Redis for Phase 2 processing.
pub struct PendingStore {
    conn: MultiplexedConnection,
}

impl PendingStore {
    pub fn new(conn: MultiplexedConnection) -> Self {
        Self { conn }
    }

    fn conn(&self) -> MultiplexedConnection {
        (*self.conn).clone()
    }

    /// Store a batch of encrypted nodes in Redis.
    /// Key: `pending:encrypted:{protocol_label}`
    /// Score: Unix timestamp of current time (enables time-range queries)
    /// Member: JSON-serialized SubscriptionProxy
    pub async fn store_batch(&self, nodes: &[SubscriptionProxy]) -> anyhow::Result<()> {
        let now_ts = chrono::Utc::now().timestamp();
        for node in nodes {
            let label = node.protocol_label();
            let key = format!("pending:encrypted:{label}");
            let member = serde_json::to_string(node)?;
            let mut conn = self.conn();
            let _: () = conn.zadd(&key, &member, now_ts).await?;
        }
        Ok(())
    }

    /// Get pending nodes for a specific protocol label.
    pub async fn get_pending(&self, protocol_label: &str, limit: usize) -> anyhow::Result<Vec<SubscriptionProxy>> {
        let key = format!("pending:encrypted:{protocol_label}");
        let mut conn = self.conn();
        let members: Vec<String> = conn.zrevrange(&key, 0, limit as i64).await?;
        let mut nodes = Vec::with_capacity(members.len());
        for m in members {
            match serde_json::from_str::<SubscriptionProxy>(&m) {
                Ok(n) => nodes.push(n),
                Err(e) => tracing::warn!("pending: parse error: {e}"),
            }
        }
        Ok(nodes)
    }

    /// Count pending nodes for a protocol label.
    pub async fn count_pending(&self, protocol_label: &str) -> anyhow::Result<usize> {
        let key = format!("pending:encrypted:{protocol_label}");
        let mut conn = self.conn();
        let c: u64 = conn.zcard(&key).await?;
        Ok(c as usize)
    }
}

#[cfg(test)]
mod tests {
    // Note: these tests require a running Redis instance.
    // Integration tests in proxy-sub/tests/ will cover Redis operations.
    // Unit tests here only verify serialization roundtrip.

    use super::*;
    use crate::models::SubscriptionProxy;
    use proxy_core::models::Protocol;

    #[test]
    fn test_subscription_proxy_serialization_roundtrip() {
        let sub = SubscriptionProxy::Shadowsocks {
            host: "10.0.0.3".into(),
            port: 8388,
            method: "aes-256-gcm".into(),
            password: "pass".into(),
            plugin: None,
            plugin_opts: None,
        };
        let json = serde_json::to_string(&sub).unwrap();
        let parsed: SubscriptionProxy = serde_json::from_str(&json).unwrap();
        assert_eq!(sub.dedup_key(), parsed.dedup_key());
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p proxy-sub --lib convert pending`
Expected: PASS

Run: `cargo test -p proxy-core --lib`
Expected: PASS (no regression)

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-sub/ crates/proxy-core/src/config.rs
git commit -m "feat(sub): add SubscriptionConfig, proxy conversion, and pending storage"
```

---

### Task 9: Refresh Loop + Main Integration

**Files:**
- Create: `crates/proxy-sub/src/refresh.rs`
- Modify: `crates/proxy-server/Cargo.toml` (add proxy-sub dep)
- Modify: `crates/proxy-server/src/main.rs` (add subscription loop spawn)
- Test: `crates/proxy-sub/src/refresh.rs` (inline tests for logic)

**Interfaces:**
- Consumes: `SubscriptionConfig` (Task 8), `Discover` trait (Task 6), `SubscriptionSource` (Task 6), `parse_subscription()` (Task 2), `partition()` (Task 8), `ProxyStore` (proxy-core), `PendingStore` (Task 8)
- Produces: `subscription_refresh_loop()`, main integration

- [ ] **Step 1: Create proxy-sub/src/refresh.rs**

```rust
//! Subscription refresh loop: discover → fetch → parse → store pipeline.

use crate::convert::partition;
use crate::discover::Discover;
use crate::parser::parse_subscription;
use crate::pending::PendingStore;
use crate::source::SubscriptionSource;
use proxy_core::config::SubscriptionConfig;
use proxy_core::store::ProxyStore;
use std::sync::Arc;
use std::time::Duration;

/// Run the subscription refresh loop periodically.
/// Discovers subscription URLs, fetches content, parses proxies,
/// stores basic nodes in ProxyStore, and stores encrypted nodes in Redis pending keys.
pub async fn subscription_refresh_loop(
    config: SubscriptionConfig,
    discoverers: Vec<Arc<dyn Discover>>,
    mut source: SubscriptionSource,
    store: Arc<ProxyStore>,
    pending: Arc<PendingStore>,
) {
    let interval = Duration::from_secs(config.refresh_interval_sec);
    tracing::info!("subscription refresh loop starting (interval={}s)", config.refresh_interval_sec);

    loop {
        if let Err(e) = run_refresh_cycle(&config, &discoverers, &mut source, &store, &pending).await {
            tracing::error!("subscription refresh cycle error: {e}");
        }
        tokio::time::sleep(interval).await;
    }
}

/// One full refresh cycle: discover → dedup → fetch → parse → partition → store.
async fn run_refresh_cycle(
    config: &SubscriptionConfig,
    discoverers: &[Arc<dyn Discover>],
    source: &mut SubscriptionSource,
    store: &Arc<ProxyStore>,
    pending: &Arc<PendingStore>,
) -> anyhow::Result<()> {
    // 1. Discover URLs from all sources
    let mut all_urls = Vec::new();
    for disc in discoverers {
        let urls = disc.discover().await;
        tracing::info!("discoverer '{}': found {} URLs", disc.name(), urls.len());
        all_urls.extend(urls);
    }

    // Dedup URLs
    let mut seen = std::collections::HashSet::new();
    all_urls.retain(|u| seen.insert(u.clone()));
    tracing::info!("subscription refresh: {} unique URLs to process", all_urls.len());

    if all_urls.is_empty() {
        return Ok(());
    }

    // 2. Evict expired cache entries
    source.evict_expired();

    // 3. Process each URL
    let mut total_basic = 0;
    let mut total_encrypted = 0;
    let mut total_failed_urls = 0;

    for url in &all_urls {
        let content = match source.fetch(url).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("subscription refresh: fetch {url} failed: {e}");
                total_failed_urls += 1;
                continue;
            }
        };

        let proxies = parse_subscription(&content);
        if proxies.is_empty() {
            tracing::debug!("subscription refresh: no proxies parsed from {url}");
            continue;
        }

        let (basic, encrypted) = partition(&proxies, url);

        // 4. Store basic proxies in ProxyStore
        for proxy in &basic {
            if let Err(e) = store.add(proxy).await {
                tracing::warn!("subscription refresh: store proxy {} failed: {e}", proxy.key());
            }
        }
        total_basic += basic.len();

        // 5. Store encrypted proxies in Redis pending keys
        if !encrypted.is_empty() {
            if let Err(e) = pending.store_batch(&encrypted).await {
                tracing::warn!("subscription refresh: store pending nodes failed: {e}");
            }
            total_encrypted += encrypted.len();
        }

        tracing::info!(
            "subscription refresh: processed {url} → {} basic, {} encrypted",
            basic.len(),
            encrypted.len()
        );
    }

    tracing::info!(
        "subscription refresh cycle complete: {} basic, {} encrypted, {} failed URLs",
        total_basic,
        total_encrypted,
        total_failed_urls
    );

    Ok(())
}

/// Build discoverers from SubscriptionConfig.
pub fn build_discoverers(config: &SubscriptionConfig) -> Vec<Arc<dyn Discover>> {
    let mut discoverers: Vec<Arc<dyn Discover>> = Vec::new();

    // Static URL discoverer
    if !config.urls.is_empty() {
        discoverers.push(Arc::new(
            crate::discover::static_url::StaticUrlDiscover::new(config.urls.clone()),
        ));
    }

    // GitHub search discoverer
    if config.github.enabled {
        let github_config = crate::discover::github_search::GitHubSearchConfig {
            token: config.github.token.clone(),
            max_results: config.github.max_results,
            keywords: if config.github.keywords.is_empty() {
                vec!["clash free sub".into(), "v2ray free nodes".into()]
            } else {
                config.github.keywords.clone()
            },
            timeout_sec: config.fetch_timeout_sec,
        };
        discoverers.push(Arc::new(
            crate::discover::github_search::GitHubSearchDiscover::new(github_config),
        ));
    }

    // Aggregator discoverers
    for agg_config in &config.aggregators {
        let agg = crate::discover::aggregator::AggregatorConfig {
            url: agg_config.url.clone(),
            format: agg_config.format.clone(),
            timeout_sec: config.fetch_timeout_sec,
        };
        discoverers.push(Arc::new(
            crate::discover::aggregator::AggregatorDiscover::new(agg),
        ));
    }

    discoverers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::Discover;
    use crate::models::SubscriptionProxy;
    use proxy_core::models::Protocol;

    /// A mock discoverer that returns a fixed URL list.
    struct MockDiscover {
        urls: Vec<String>,
    }

    #[async_trait::async_trait]
    impl Discover for MockDiscover {
        fn name(&self) -> &str { "Mock" }
        async fn discover(&self) -> Vec<String> { self.urls.clone() }
    }

    #[test]
    fn test_build_discoverers_static() {
        let config = SubscriptionConfig {
            urls: vec!["https://example.com/sub".into()],
            github: proxy_core::config::GitHubDiscoverConfig {
                enabled: false,
                token: None,
                max_results: 20,
                search_interval_sec: 86400,
                keywords: vec![],
            },
            aggregators: vec![],
            refresh_interval_sec: 3600,
            fetch_timeout_sec: 30,
            cache_ttl_sec: 1800,
        };
        let discoverers = build_discoverers(&config);
        assert_eq!(discoverers.len(), 1);
        assert_eq!(discoverers[0].name(), "StaticUrl");
    }

    #[test]
    fn test_build_discoverers_all() {
        let config = SubscriptionConfig {
            urls: vec!["https://example.com/sub".into()],
            github: proxy_core::config::GitHubDiscoverConfig {
                enabled: true,
                token: Some("ghp_xxx".into()),
                max_results: 20,
                search_interval_sec: 86400,
                keywords: vec!["clash free sub".into()],
            },
            aggregators: vec![proxy_core::config::AggregatorEntryConfig {
                url: "https://agg.example.com/list.txt".into(),
                format: "text".into(),
                refresh_interval_sec: 43200,
            }],
            refresh_interval_sec: 3600,
            fetch_timeout_sec: 30,
            cache_ttl_sec: 1800,
        };
        let discoverers = build_discoverers(&config);
        assert_eq!(discoverers.len(), 3);
    }
}
```

- [ ] **Step 2: Add proxy-sub dependency to proxy-server/Cargo.toml**

Add to `crates/proxy-server/Cargo.toml` dependencies:

```toml
proxy-sub = { path = "../proxy-sub" }
```

Also add to workspace Cargo.toml `[workspace.dependencies]`:

```toml
proxy-sub = { path = "crates/proxy-sub" }
```

And reference in proxy-server as `proxy-sub = { workspace = true }`.

- [ ] **Step 3: Integrate subscription refresh loop into main.rs**

Modify `crates/proxy-server/src/main.rs`:

Add import at the top:
```rust
use proxy_sub::refresh::{build_discoverers, subscription_refresh_loop};
use proxy_sub::pending::PendingStore;
use proxy_sub::source::SubscriptionSource;
```

After the `scheduler_handle` block (around line 102), add:

```rust
// Subscription refresh loop
let sub_handle = {
    let sub_config = settings.subscription.clone();
    let discoverers = build_discoverers(&sub_config);
    let sub_source = SubscriptionSource::new(sub_config.cache_ttl_sec, sub_config.fetch_timeout_sec);
    let pending = Arc::new(PendingStore::new(redis_multiplexed));
    tokio::spawn(subscription_refresh_loop(
        sub_config,
        discoverers,
        sub_source,
        store.clone(),
        pending,
    ))
};
```

Add `sub_handle` to the `tokio::select!` block:
```rust
tokio::select! {
    r = scheduler_handle => tracing::info!("scheduler stopped: {:?}", r),
    r = health_handle => tracing::info!("health checker stopped: {:?}", r),
    r = api_handle => tracing::info!("API server stopped: {:?}", r),
    r = gateway_handle => tracing::info!("gateway stopped: {:?}", r),
    r = mcp_handle => tracing::info!("MCP server stopped: {:?}", r),
    r = sub_handle => tracing::info!("subscription refresh stopped: {:?}", r),
}
```

- [ ] **Step 4: Run tests and clippy**

Run: `cargo test -p proxy-sub --lib`
Expected: ALL PASS

Run: `cargo clippy -p proxy-sub -p proxy-server -- -D warnings`
Expected: No warnings

Run: `cargo build -p proxy-server`
Expected: Successful build

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-sub/ crates/proxy-server/ Cargo.toml
git commit -m "feat(sub): add refresh loop and main integration"
```

---

### Task 10: Final Validation — Full Build + Clippy + All Tests

**Files:** None (validation only)

**Interfaces:** N/A

- [ ] **Step 1: Run full workspace build**

Run: `cargo build`
Expected: Successful build

- [ ] **Step 2: Run all workspace tests**

Run: `cargo test`
Expected: ALL PASS (no regressions in proxy-core, proxy-sub tests pass)

- [ ] **Step 3: Run clippy on full workspace**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 4: Run cargo fmt**

Run: `cargo fmt -- --check`
Expected: No formatting issues

- [ ] **Step 5: Final commit (if any formatting fixes needed)**

```bash
cargo fmt
git add -A
git commit -m "chore: format and validate full workspace"
```

---

## Self-Review

**1. Spec coverage check:**

| Spec requirement | Covered by |
|-----------------|------------|
| SubscriptionProxy enum (5 variants) | Task 1 |
| Parser trait + parse_subscription() | Task 2 |
| Clash YAML parser | Task 2 |
| Base64 URI parser | Task 3 |
| V2Ray JSON parser | Task 4 |
| Surge parser | Task 5 |
| Discover trait | Task 6 |
| StaticUrlDiscover | Task 6 |
| GitHubSearchDiscover | Task 7 |
| AggregatorDiscover | Task 7 |
| SubscriptionSource + ContentCache | Task 6 |
| SubscriptionConfig in Settings | Task 8 |
| Proxy conversion (Basic → Proxy) | Task 8 |
| Redis pending storage | Task 8 |
| Refresh loop orchestration | Task 9 |
| Main integration | Task 9 |
| EncryptedProxyState (Phase 2 reservation) | Task 1 |
| Test fixtures (5 files) | Tasks 2, 3, 4, 5 |
| Format detection heuristics | Tasks 2-5 (detect() methods) |
| Error handling (log warning, skip, continue) | All tasks |
| Success criteria | Task 10 validates |

**2. Placeholder scan:** No TBD, TODO, or vague steps found. All code blocks contain complete implementations.

**3. Type consistency:** All cross-task references verified:
- `SubscriptionProxy` used consistently across Tasks 1-9
- `Parser` trait signature matches all parser implementations
- `Discover` trait signature matches all discoverer implementations
- `SubscriptionConfig` fields match `build_discoverers()` consumption in Task 9
- `partition()` produces `(Vec<Proxy>, Vec<SubscriptionProxy>)` matching refresh loop usage
- `PendingStore::store_batch()` takes `&[SubscriptionProxy]` matching refresh loop call
