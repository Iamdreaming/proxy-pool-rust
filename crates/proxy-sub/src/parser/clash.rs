//! Clash YAML subscription parser.
//!
//! Extracts the `proxies:` array from Clash/Mihomo YAML config.
//! Supported types: socks5, http, ss, vmess, trojan, vless.
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
    #[serde(default, deserialize_with = "de_port")]
    port: u16,
    // SS fields
    #[serde(default)]
    cipher: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    plugin: Option<String>,
    // Clash emits plugin-opts as a map, not a string; accept any YAML value.
    #[serde(default, rename = "plugin-opts")]
    plugin_opts: Option<serde_yaml::Value>,
    // VMess fields
    #[serde(default)]
    uuid: String,
    #[serde(default, rename = "alterId")]
    alter_id: u32,
    #[serde(default)]
    encryption: Option<String>,
    #[serde(default)]
    flow: Option<String>,
    #[serde(default)]
    network: String,
    #[serde(default)]
    sni: Option<String>,
    #[serde(default)]
    servername: Option<String>,
    #[serde(default, rename = "client-fingerprint")]
    client_fingerprint: Option<String>,
    // WS opts
    #[serde(default, rename = "ws-opts")]
    ws_opts: Option<WsOpts>,
    #[serde(default, rename = "grpc-opts")]
    grpc_opts: Option<GrpcOpts>,
    #[serde(default, rename = "reality-opts")]
    reality_opts: Option<RealityOpts>,
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

#[derive(Debug, Deserialize)]
struct GrpcOpts {
    #[serde(default, rename = "grpc-service-name")]
    grpc_service_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RealityOpts {
    #[serde(default, rename = "public-key")]
    public_key: Option<String>,
    #[serde(default, rename = "short-id")]
    short_id: Option<String>,
}

/// Top-level Clash YAML with `proxies` key.
#[derive(Debug, Deserialize)]
struct ClashConfig {
    #[serde(default)]
    proxies: Vec<ClashProxyEntry>,
}

/// Deserialize a port that may be a YAML integer or a quoted string.
fn de_port<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    match serde_yaml::Value::deserialize(deserializer)? {
        serde_yaml::Value::Number(n) => n
            .as_u64()
            .and_then(|v| u16::try_from(v).ok())
            .ok_or_else(|| serde::de::Error::custom("port out of range")),
        serde_yaml::Value::String(s) => s.trim().parse().map_err(serde::de::Error::custom),
        serde_yaml::Value::Null => Ok(0),
        other => Err(serde::de::Error::custom(format!(
            "invalid port value: {other:?}"
        ))),
    }
}

/// Flatten a Clash `plugin-opts` map into the SIP003 `k=v;k=v` string form.
///
/// Clash represents plugin options as a YAML map (e.g. `{mode: websocket,
/// host: a.com}`); the pool model stores the standard semicolon-delimited
/// string. A plain string value is passed through unchanged.
fn flatten_plugin_opts(value: Option<&serde_yaml::Value>) -> Option<String> {
    match value? {
        serde_yaml::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_yaml::Value::Mapping(map) => {
            let parts: Vec<String> = map
                .iter()
                .filter_map(|(k, v)| {
                    let key = k.as_str()?;
                    let val = match v {
                        serde_yaml::Value::String(s) => s.clone(),
                        serde_yaml::Value::Bool(b) => b.to_string(),
                        serde_yaml::Value::Number(n) => n.to_string(),
                        _ => return None,
                    };
                    Some(format!("{key}={val}"))
                })
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(";"))
            }
        }
        _ => None,
    }
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
                        plugin_opts: flatten_plugin_opts(entry.plugin_opts.as_ref()),
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
                    "vless" => {
                        let (path, host_header) = match &entry.ws_opts {
                            Some(ws) => (
                                ws.path.clone(),
                                ws.headers.as_ref().and_then(|h| h.get("Host").cloned()),
                            ),
                            None => (None, None),
                        };
                        let network = if entry.network.is_empty() {
                            "tcp".into()
                        } else {
                            entry.network.clone()
                        };
                        let security = if entry.reality_opts.is_some() {
                            Some("reality".into())
                        } else if entry.tls == Some(true) {
                            Some("tls".into())
                        } else {
                            None
                        };
                        SubscriptionProxy::Vless {
                            host: entry.server.clone(),
                            port: entry.port,
                            uuid: entry.uuid.clone(),
                            encryption: entry.encryption.clone().unwrap_or_else(|| "none".into()),
                            flow: entry.flow.clone(),
                            network,
                            security,
                            sni: entry.servername.clone().or_else(|| entry.sni.clone()),
                            host_header,
                            path,
                            service_name: entry
                                .grpc_opts
                                .as_ref()
                                .and_then(|opts| opts.grpc_service_name.clone()),
                            fingerprint: entry.client_fingerprint.clone(),
                            public_key: entry
                                .reality_opts
                                .as_ref()
                                .and_then(|opts| opts.public_key.clone()),
                            short_id: entry
                                .reality_opts
                                .as_ref()
                                .and_then(|opts| opts.short_id.clone()),
                            spider_x: None,
                        }
                    }
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

    #[test]
    fn test_clash_parse_vless_reality() {
        let parser = ClashParser;
        let content = r#"
proxies:
  - name: vless-reality
    type: vless
    server: reality.example.com
    port: 443
    uuid: 550e8400-e29b-41d4-a716-446655440000
    network: grpc
    flow: xtls-rprx-vision
    servername: www.microsoft.com
    client-fingerprint: chrome
    grpc-opts:
      grpc-service-name: grpc-service
    reality-opts:
      public-key: pub-key
      short-id: abcd
"#;
        let proxies = parser.parse(content);
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Vless {
            host,
            port,
            uuid,
            encryption,
            flow,
            network,
            security,
            sni,
            service_name,
            fingerprint,
            public_key,
            short_id,
            ..
        } = &proxies[0]
        {
            assert_eq!(host, "reality.example.com");
            assert_eq!(*port, 443);
            assert_eq!(uuid, "550e8400-e29b-41d4-a716-446655440000");
            assert_eq!(encryption, "none");
            assert_eq!(flow.as_deref(), Some("xtls-rprx-vision"));
            assert_eq!(network, "grpc");
            assert_eq!(security.as_deref(), Some("reality"));
            assert_eq!(sni.as_deref(), Some("www.microsoft.com"));
            assert_eq!(service_name.as_deref(), Some("grpc-service"));
            assert_eq!(fingerprint.as_deref(), Some("chrome"));
            assert_eq!(public_key.as_deref(), Some("pub-key"));
            assert_eq!(short_id.as_deref(), Some("abcd"));
        } else {
            panic!("Expected Vless, got {:?}", proxies[0]);
        }
    }

    #[test]
    fn test_clash_ss_map_plugin_opts_and_quoted_port() {
        let parser = ClashParser;
        // plugin-opts is a map and port is quoted — both previously failed the
        // whole-document deserialization, yielding zero proxies.
        let content = r#"
proxies:
  - name: ss-plugin
    type: ss
    server: 10.0.0.9
    port: "8443"
    cipher: aes-256-gcm
    password: secret
    plugin: obfs
    plugin-opts:
      mode: websocket
      host: cdn.example.com
"#;
        let proxies = parser.parse(content);
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Shadowsocks {
            host,
            port,
            plugin,
            plugin_opts,
            ..
        } = &proxies[0]
        {
            assert_eq!(host, "10.0.0.9");
            assert_eq!(*port, 8443);
            assert_eq!(plugin.as_deref(), Some("obfs"));
            let opts = plugin_opts.as_deref().unwrap();
            assert!(opts.contains("mode=websocket"));
            assert!(opts.contains("host=cdn.example.com"));
        } else {
            panic!("Expected Shadowsocks, got {:?}", proxies[0]);
        }
    }
}
