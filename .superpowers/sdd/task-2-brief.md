# Task 2: Parser Trait + Clash YAML Parser

**Files:**
- Create: `crates/proxy-sub/src/parser/mod.rs`
- Create: `crates/proxy-sub/src/parser/clash.rs`
- Create: `crates/proxy-sub/tests/fixtures/clash_sample.yaml`
- Test: `crates/proxy-sub/src/parser/clash.rs` (inline tests)

**Interfaces:**
- Consumes: `SubscriptionProxy` (from Task 1, in `crate::models`)
- Produces: `Parser` trait, `ClashParser`, `parse_subscription()` entrypoint

## Step 1: Create test fixture

Create `crates/proxy-sub/tests/fixtures/clash_sample.yaml`:

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

## Step 2: Create parser/mod.rs with Parser trait and parse_subscription()

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

## Step 3: Implement ClashParser

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
    #[serde(rename = "type")]
    proxy_type: String,
    #[serde(default)]
    server: String,
    #[serde(default)]
    port: u16,
    // SS fields
    #[serde(default)]
    cipher: String,
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
    #[serde(default)]
    network: String,
    #[serde(default)]
    sni: Option<String>,
    // WS opts
    #[serde(default, rename = "ws-opts")]
    ws_opts: Option<WsOpts>,
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
        if !content.contains("proxies:") {
            return false;
        }
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
                let type_lower = entry.proxy_type.to_ascii_lowercase();
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
                        method: entry.cipher.clone(),
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
                            security: entry.cipher.clone(),
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
                            entry.proxy_type, entry.server, entry.port
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

## Step 4: Run tests

Run: `cargo test -p proxy-sub --lib`
Expected: ALL PASS

Run: `cargo clippy -p proxy-sub -- -D warnings`
Expected: No warnings

## Step 5: Commit

```bash
git add crates/proxy-sub/
git commit -m "feat(sub): add Parser trait and Clash YAML parser"
```
