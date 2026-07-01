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
    #[allow(dead_code)]
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
            .map(|entry| {
                let type_lower = entry.proxy_type.to_ascii_lowercase();
                match type_lower.as_str() {
                    "socks5" => SubscriptionProxy::Basic {
                        host: entry.server.clone(),
                        port: entry.port,
                        protocol: Protocol::Socks5,
                    },
                    "http" => SubscriptionProxy::Basic {
                        host: entry.server.clone(),
                        port: entry.port,
                        protocol: if entry.tls == Some(true) {
                            Protocol::Https
                        } else {
                            Protocol::Http
                        },
                    },
                    "ss" => SubscriptionProxy::Shadowsocks {
                        host: entry.server.clone(),
                        port: entry.port,
                        method: entry.cipher.clone(),
                        password: entry.password.clone(),
                        plugin: entry.plugin.clone(),
                        plugin_opts: entry.plugin_opts.clone(),
                    },
                    "vmess" => {
                        let (path, host_header) = match &entry.ws_opts {
                            Some(ws) => (
                                ws.path.clone(),
                                ws.headers.as_ref().and_then(|h| h.get("Host").cloned()),
                            ),
                            None => (None, None),
                        };
                        SubscriptionProxy::Vmess {
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
                        }
                    }
                    "trojan" => SubscriptionProxy::Trojan {
                        host: entry.server.clone(),
                        port: entry.port,
                        password: entry.password.clone(),
                        sni: entry.sni.clone(),
                        network: if entry.network.is_empty() {
                            None
                        } else {
                            Some(entry.network.clone())
                        },
                    },
                    _ => SubscriptionProxy::Unknown {
                        raw_config: format!(
                            "type={}, server={}, port={}",
                            entry.proxy_type, entry.server, entry.port
                        ),
                    },
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

        // socks5 -> Basic
        assert!(proxies[0].is_direct_usable());
        if let SubscriptionProxy::Basic {
            host,
            port,
            protocol,
            ..
        } = &proxies[0]
        {
            assert_eq!(host, "10.0.0.1");
            assert_eq!(*port, 1080);
            assert_eq!(*protocol, Protocol::Socks5);
        }

        // ss -> Shadowsocks
        if let SubscriptionProxy::Shadowsocks { host, method, .. } = &proxies[2] {
            assert_eq!(host, "10.0.0.3");
            assert_eq!(method, "aes-256-gcm");
        }

        // vmess -> Vmess
        if let SubscriptionProxy::Vmess { uuid, network, .. } = &proxies[3] {
            assert_eq!(uuid, "a3482e88-686a-4a58-8126-99c9df64b7bf");
            assert_eq!(network, "ws");
        }

        // hysteria2 -> Unknown
        assert!(matches!(&proxies[5], SubscriptionProxy::Unknown { .. }));
    }
}
