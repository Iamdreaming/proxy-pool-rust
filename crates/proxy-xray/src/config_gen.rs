//! Config generation for xray-core.
//!
//! Produces JSON config fragments for xray's inbound/outbound/routing rules.
//! The JSON approach is used as the primary mechanism — xray-core accepts JSON
//! config in its gRPC `HandlerService` via `TypedMessage` wrapping.

use proxy_sub::models::SubscriptionProxy;
use serde_json::{Value, json};

/// A complete set of xray JSON config fragments for a single node.
pub struct XrayNodeConfig {
    /// Node tag: "{protocol_label}-{host}-{port}"
    pub tag: String,
    /// SOCKS5 inbound JSON config.
    pub inbound_json: Value,
    /// Outbound JSON config (protocol-specific).
    pub outbound_json: Value,
    /// Routing rule JSON linking inbound to outbound.
    pub routing_rule_json: Value,
}

/// Stateless config generator: converts `SubscriptionProxy` nodes into xray JSON.
pub struct ConfigGenerator;

impl ConfigGenerator {
    /// Generate a complete `XrayNodeConfig` for the given node and local port.
    ///
    /// Returns `None` for `Basic` and `Unknown` variants (no xray outbound needed).
    pub fn generate(node: &SubscriptionProxy, local_socks5_port: u16) -> Option<XrayNodeConfig> {
        let tag = node_tag(node);
        let inbound_tag = format!("in-{tag}");
        let outbound_tag = format!("out-{tag}");

        let outbound_json = generate_outbound_json(node, &outbound_tag)?;
        let inbound_json = generate_inbound_json(&inbound_tag, local_socks5_port);
        let routing_rule_json = generate_routing_rule_json(&inbound_tag, &outbound_tag);

        Some(XrayNodeConfig {
            tag,
            inbound_json,
            outbound_json,
            routing_rule_json,
        })
    }

    /// Generate the bootstrap xray JSON config used to start the process.
    ///
    /// This config sets up the gRPC API listener and a default freedom outbound
    /// so that xray-core can accept gRPC HandlerService calls for dynamic inbound/
    /// outbound addition.
    pub fn generate_bootstrap_config(api_port: u16) -> String {
        let config = json!({
            "api": {
                "tag": "api",
                "services": ["HandlerService", "RoutingService"]
            },
            "inbounds": [
                {
                    "tag": "api",
                    "protocol": "dokodemo-door",
                    "listen": "127.0.0.1",
                    "port": api_port,
                    "settings": {
                        "address": "127.0.0.1"
                    }
                }
            ],
            "outbounds": [
                {
                    "tag": "direct",
                    "protocol": "freedom",
                    "settings": {}
                }
            ],
            "routing": {
                "rules": [
                    {
                        "type": "field",
                        "inboundTag": ["api"],
                        "outboundTag": "api"
                    }
                ]
            },
            "stats": {},
            "policy": {
                "levels": {
                    "0": {
                        "statsUplink": true,
                        "statsDownlink": true
                    }
                },
                "system": {
                    "statsInboundUplink": true,
                    "statsInboundDownlink": true,
                    "statsOutboundUplink": true,
                    "statsOutboundDownlink": true
                }
            }
        });
        serde_json::to_string_pretty(&config).unwrap_or_default()
    }

    /// Write the bootstrap config to a file.
    pub fn write_bootstrap_config(path: &std::path::Path, api_port: u16) -> anyhow::Result<()> {
        let json = Self::generate_bootstrap_config(api_port);
        std::fs::write(path, json)?;
        Ok(())
    }
}

/// Compute the unique tag for a node: "{protocol_label}-{host}-{port}".
pub fn node_tag(node: &SubscriptionProxy) -> String {
    format!(
        "{}-{}-{}",
        node.protocol_label(),
        node.host().unwrap_or("unknown"),
        node.port().unwrap_or(0)
    )
}

/// Generate a SOCKS5 inbound JSON config.
///
/// ```json
/// {
///   "tag": "in-ss-1.2.3.4-8388",
///   "protocol": "socks",
///   "port": 20000,
///   "listen": "127.0.0.1",
///   "settings": { "auth": "noauth", "udp": true }
/// }
/// ```
pub fn generate_inbound_json(tag: &str, local_port: u16) -> Value {
    json!({
        "tag": tag,
        "protocol": "socks",
        "port": local_port,
        "listen": "127.0.0.1",
        "settings": {
            "auth": "noauth",
            "udp": true
        }
    })
}

/// Generate a routing rule JSON that links an inbound tag to an outbound tag.
///
/// ```json
/// {
///   "type": "field",
///   "inboundTag": ["in-ss-1.2.3.4-8388"],
///   "outboundTag": "out-ss-1.2.3.4-8388"
/// }
/// ```
pub fn generate_routing_rule_json(inbound_tag: &str, outbound_tag: &str) -> Value {
    json!({
        "type": "field",
        "ruleTag": routing_rule_tag(inbound_tag),
        "inboundTag": [inbound_tag],
        "outboundTag": outbound_tag
    })
}

/// The `ruleTag` used to identify (and later remove) a node's routing rule.
pub fn routing_rule_tag(inbound_tag: &str) -> String {
    format!("rule-{inbound_tag}")
}

/// Shadowsocks ciphers that xray-core can build an outbound for (AEAD, the
/// 2022 edition, and `none`/`plain`).
///
/// Legacy stream ciphers such as `aes-256-cfb`, `aes-128-ctr`, and `rc4-md5`
/// were removed from xray-core and make `xray api ado` fail with
/// "unknown cipher method", so nodes using them can never be activated.
pub fn shadowsocks_cipher_supported(method: &str) -> bool {
    matches!(
        method.trim().to_ascii_lowercase().as_str(),
        "aes-256-gcm"
            | "aes-128-gcm"
            | "chacha20-poly1305"
            | "chacha20-ietf-poly1305"
            | "xchacha20-poly1305"
            | "xchacha20-ietf-poly1305"
            | "2022-blake3-aes-128-gcm"
            | "2022-blake3-aes-256-gcm"
            | "2022-blake3-chacha20-poly1305"
            | "none"
            | "plain"
    )
}

/// Whether xray-core can build an outbound for this node.
///
/// Returns `false` for `Basic`/`Unknown` (no xray outbound) and for Shadowsocks
/// nodes whose cipher xray does not support, so callers can skip them before
/// spending an activation attempt.
pub fn is_xray_activatable(node: &SubscriptionProxy) -> bool {
    match node {
        SubscriptionProxy::Shadowsocks { method, .. } => shadowsocks_cipher_supported(method),
        SubscriptionProxy::Vmess { .. }
        | SubscriptionProxy::Trojan { .. }
        | SubscriptionProxy::Vless { .. } => true,
        SubscriptionProxy::Basic { .. } | SubscriptionProxy::Unknown { .. } => false,
    }
}

/// Generate an outbound JSON config for the given `SubscriptionProxy`.
///
/// Returns `None` for `Basic` and `Unknown` variants.
pub fn generate_outbound_json(node: &SubscriptionProxy, tag: &str) -> Option<Value> {
    match node {
        SubscriptionProxy::Shadowsocks {
            host,
            port,
            method,
            password,
            ..
        } => Some(json!({
            "tag": tag,
            "protocol": "shadowsocks",
            "settings": {
                "servers": [{
                    "address": host,
                    "port": port,
                    "method": method,
                    "password": password
                }]
            },
            "streamSettings": {
                "network": "tcp"
            }
        })),
        SubscriptionProxy::Vmess {
            host,
            port,
            uuid,
            alter_id,
            security,
            network,
            path,
            host_header,
            sni,
        } => {
            let stream_settings = build_vmess_stream_settings(network, path, host_header, sni);
            Some(json!({
                "tag": tag,
                "protocol": "vmess",
                "settings": {
                    "vnext": [{
                        "address": host,
                        "port": port,
                        "users": [{
                            "id": uuid,
                            "alterId": alter_id,
                            "security": security
                        }]
                    }]
                },
                "streamSettings": stream_settings
            }))
        }
        SubscriptionProxy::Vless {
            host,
            port,
            uuid,
            encryption,
            flow,
            network,
            security,
            sni,
            host_header,
            path,
            service_name,
            fingerprint,
            public_key,
            short_id,
            spider_x,
        } => {
            let stream_settings = build_stream_settings(StreamSettingsInput {
                network,
                path,
                host_header,
                service_name,
                security,
                sni,
                fingerprint,
                public_key,
                short_id,
                spider_x,
            });
            let mut settings = json!({
                "address": host,
                "port": port,
                "id": uuid,
                "encryption": encryption
            });
            if let Some(flow_value) = flow {
                settings["flow"] = json!(flow_value);
            }
            Some(json!({
                "tag": tag,
                "protocol": "vless",
                "settings": settings,
                "streamSettings": stream_settings
            }))
        }
        SubscriptionProxy::Trojan {
            host,
            port,
            password,
            sni,
            network,
        } => {
            // xray-core 1.8.24+ removed "http" transport; map to "xhttp".
            let net = match network.as_deref().unwrap_or("tcp") {
                "http" | "h2" => "xhttp",
                other => other,
            };
            let mut stream = json!({
                "network": net
            });

            if let Some(sni_val) = sni {
                stream["security"] = json!("tls");
                stream["tlsSettings"] = json!({
                    "serverName": sni_val
                });
            }

            Some(json!({
                "tag": tag,
                "protocol": "trojan",
                "settings": {
                    "servers": [{
                        "address": host,
                        "port": port,
                        "password": password
                    }]
                },
                "streamSettings": stream
            }))
        }
        // Basic and Unknown nodes do not require xray outbounds.
        SubscriptionProxy::Basic { .. } | SubscriptionProxy::Unknown { .. } => None,
    }
}

/// Build VMess `streamSettings` JSON based on transport and TLS options.
///
/// Network mapping:
/// - `"ws"` -> generate `wsSettings` from path/host_header
/// - `"grpc"` -> generate `grpcSettings` from path (as serviceName)
/// - `"tcp"` -> no extra transport settings
///
/// If `sni` is present, sets `"security": "tls"` with `tlsSettings.serverName`.
fn build_vmess_stream_settings(
    network: &str,
    path: &Option<String>,
    host_header: &Option<String>,
    sni: &Option<String>,
) -> Value {
    let grpc_service_name = if network == "grpc" {
        path.clone()
    } else {
        None
    };
    build_stream_settings(StreamSettingsInput {
        network,
        path,
        host_header,
        service_name: &grpc_service_name,
        security: &None,
        sni,
        fingerprint: &None,
        public_key: &None,
        short_id: &None,
        spider_x: &None,
    })
}

struct StreamSettingsInput<'a> {
    network: &'a str,
    path: &'a Option<String>,
    host_header: &'a Option<String>,
    service_name: &'a Option<String>,
    security: &'a Option<String>,
    sni: &'a Option<String>,
    fingerprint: &'a Option<String>,
    public_key: &'a Option<String>,
    short_id: &'a Option<String>,
    spider_x: &'a Option<String>,
}

fn build_stream_settings(input: StreamSettingsInput<'_>) -> Value {
    // xray-core 1.8.24+ removed the "http" transport; map to "xhttp" (splithttp).
    let effective_network = match input.network {
        "http" | "h2" => "xhttp",
        other => other,
    };
    let mut stream = json!({ "network": effective_network });

    match effective_network {
        "ws" => {
            let mut ws = json!({});
            if let Some(p) = input.path {
                ws["path"] = json!(p);
            }
            if let Some(h) = input.host_header {
                ws["headers"] = json!({ "Host": h });
            }
            stream["wsSettings"] = ws;
        }
        "grpc" => {
            if let Some(p) = input.service_name.as_ref().or(input.path.as_ref()) {
                stream["grpcSettings"] = json!({
                    "serviceName": p
                });
            }
        }
        _ => {
            // "tcp" or other: no extra transport settings needed
        }
    }

    let security_value = input
        .security
        .as_deref()
        .filter(|value| !value.is_empty() && *value != "none")
        .or_else(|| input.sni.as_ref().map(|_| "tls"));

    match security_value {
        Some("tls") => {
            stream["security"] = json!("tls");
            let mut tls = json!({});
            if let Some(sni_val) = input.sni {
                tls["serverName"] = json!(sni_val);
            }
            if let Some(fp) = input.fingerprint {
                tls["fingerprint"] = json!(fp);
            }
            stream["tlsSettings"] = tls;
        }
        Some("reality") => {
            stream["security"] = json!("reality");
            let mut reality = json!({});
            if let Some(sni_val) = input.sni {
                reality["serverName"] = json!(sni_val);
            }
            if let Some(fp) = input.fingerprint {
                reality["fingerprint"] = json!(fp);
            }
            if let Some(pk) = input.public_key {
                reality["password"] = json!(pk);
            }
            if let Some(sid) = input.short_id {
                reality["shortId"] = json!(sid);
            }
            if let Some(spx) = input.spider_x {
                reality["spiderX"] = json!(spx);
            }
            stream["realitySettings"] = reality;
        }
        Some(other) => {
            stream["security"] = json!(other);
        }
        None => {}
    }

    stream
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxy_core::models::Protocol;

    #[test]
    fn test_node_tag() {
        let ss = SubscriptionProxy::Shadowsocks {
            host: "1.2.3.4".into(),
            port: 8388,
            method: "aes-256-gcm".into(),
            password: "pass".into(),
            plugin: None,
            plugin_opts: None,
        };
        assert_eq!(node_tag(&ss), "ss-1.2.3.4-8388");
    }

    #[test]
    fn test_generate_shadowsocks() {
        let ss = SubscriptionProxy::Shadowsocks {
            host: "1.2.3.4".into(),
            port: 8388,
            method: "aes-256-gcm".into(),
            password: "mypassword".into(),
            plugin: None,
            plugin_opts: None,
        };
        let config = ConfigGenerator::generate(&ss, 20000).unwrap();
        assert_eq!(config.tag, "ss-1.2.3.4-8388");

        let ob = &config.outbound_json;
        assert_eq!(ob["protocol"], "shadowsocks");
        assert_eq!(ob["tag"], "out-ss-1.2.3.4-8388");
        assert_eq!(ob["settings"]["servers"][0]["address"], "1.2.3.4");
        assert_eq!(ob["settings"]["servers"][0]["port"], 8388);
        assert_eq!(ob["settings"]["servers"][0]["method"], "aes-256-gcm");
        assert_eq!(ob["settings"]["servers"][0]["password"], "mypassword");
        assert_eq!(ob["streamSettings"]["network"], "tcp");

        let ib = &config.inbound_json;
        assert_eq!(ib["tag"], "in-ss-1.2.3.4-8388");
        assert_eq!(ib["protocol"], "socks");
        assert_eq!(ib["port"], 20000);

        let rr = &config.routing_rule_json;
        assert_eq!(rr["type"], "field");
        assert_eq!(rr["outboundTag"], "out-ss-1.2.3.4-8388");
    }

    #[test]
    fn test_generate_vmess_ws_tls() {
        let vmess = SubscriptionProxy::Vmess {
            host: "5.6.7.8".into(),
            port: 443,
            uuid: "uuid-string".into(),
            alter_id: 0,
            security: "auto".into(),
            network: "ws".into(),
            path: Some("/v2ray".into()),
            host_header: Some("vmess.example.com".into()),
            sni: Some("vmess.example.com".into()),
        };
        let config = ConfigGenerator::generate(&vmess, 20001).unwrap();
        assert_eq!(config.tag, "vmess-5.6.7.8-443");

        let ob = &config.outbound_json;
        assert_eq!(ob["protocol"], "vmess");
        assert_eq!(ob["settings"]["vnext"][0]["address"], "5.6.7.8");
        assert_eq!(ob["settings"]["vnext"][0]["users"][0]["id"], "uuid-string");
        assert_eq!(ob["streamSettings"]["network"], "ws");
        assert_eq!(ob["streamSettings"]["wsSettings"]["path"], "/v2ray");
        assert_eq!(
            ob["streamSettings"]["wsSettings"]["headers"]["Host"],
            "vmess.example.com"
        );
        assert_eq!(ob["streamSettings"]["security"], "tls");
        assert_eq!(
            ob["streamSettings"]["tlsSettings"]["serverName"],
            "vmess.example.com"
        );
    }

    #[test]
    fn test_generate_vmess_grpc() {
        let vmess = SubscriptionProxy::Vmess {
            host: "5.6.7.8".into(),
            port: 443,
            uuid: "uuid-string".into(),
            alter_id: 0,
            security: "auto".into(),
            network: "grpc".into(),
            path: Some("grpc-service".into()),
            host_header: None,
            sni: None,
        };
        let config = ConfigGenerator::generate(&vmess, 20002).unwrap();
        let ob = &config.outbound_json;
        assert_eq!(ob["streamSettings"]["network"], "grpc");
        assert_eq!(
            ob["streamSettings"]["grpcSettings"]["serviceName"],
            "grpc-service"
        );
    }

    #[test]
    fn test_generate_vless_ws_tls() {
        let vless = SubscriptionProxy::Vless {
            host: "vless.example.com".into(),
            port: 443,
            uuid: "550e8400-e29b-41d4-a716-446655440000".into(),
            encryption: "none".into(),
            flow: None,
            network: "ws".into(),
            security: Some("tls".into()),
            sni: Some("sni.example.com".into()),
            host_header: Some("cdn.example.com".into()),
            path: Some("/ws".into()),
            service_name: None,
            fingerprint: Some("chrome".into()),
            public_key: None,
            short_id: None,
            spider_x: None,
        };
        let config = ConfigGenerator::generate(&vless, 20005).unwrap();
        assert_eq!(config.tag, "vless-vless.example.com-443");

        let ob = &config.outbound_json;
        assert_eq!(ob["protocol"], "vless");
        assert_eq!(ob["settings"]["address"], "vless.example.com");
        assert_eq!(ob["settings"]["port"], 443);
        assert_eq!(ob["settings"]["id"], "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(ob["settings"]["encryption"], "none");
        assert_eq!(ob["streamSettings"]["network"], "ws");
        assert_eq!(ob["streamSettings"]["wsSettings"]["path"], "/ws");
        assert_eq!(
            ob["streamSettings"]["wsSettings"]["headers"]["Host"],
            "cdn.example.com"
        );
        assert_eq!(ob["streamSettings"]["security"], "tls");
        assert_eq!(
            ob["streamSettings"]["tlsSettings"]["serverName"],
            "sni.example.com"
        );
        assert_eq!(ob["streamSettings"]["tlsSettings"]["fingerprint"], "chrome");
    }

    #[test]
    fn test_generate_vless_reality() {
        let vless = SubscriptionProxy::Vless {
            host: "reality.example.com".into(),
            port: 443,
            uuid: "uid".into(),
            encryption: "none".into(),
            flow: Some("xtls-rprx-vision".into()),
            network: "tcp".into(),
            security: Some("reality".into()),
            sni: Some("www.microsoft.com".into()),
            host_header: None,
            path: None,
            service_name: None,
            fingerprint: Some("chrome".into()),
            public_key: Some("pub-key".into()),
            short_id: Some("abcd".into()),
            spider_x: Some("/".into()),
        };
        let config = ConfigGenerator::generate(&vless, 20006).unwrap();
        let ob = &config.outbound_json;

        assert_eq!(ob["protocol"], "vless");
        assert_eq!(ob["settings"]["flow"], "xtls-rprx-vision");
        assert_eq!(ob["streamSettings"]["network"], "tcp");
        assert_eq!(ob["streamSettings"]["security"], "reality");
        assert_eq!(
            ob["streamSettings"]["realitySettings"]["serverName"],
            "www.microsoft.com"
        );
        assert_eq!(
            ob["streamSettings"]["realitySettings"]["fingerprint"],
            "chrome"
        );
        assert_eq!(
            ob["streamSettings"]["realitySettings"]["password"],
            "pub-key"
        );
        assert_eq!(ob["streamSettings"]["realitySettings"]["shortId"], "abcd");
        assert_eq!(ob["streamSettings"]["realitySettings"]["spiderX"], "/");
    }

    #[test]
    fn test_generate_trojan_tls() {
        let trojan = SubscriptionProxy::Trojan {
            host: "9.10.11.12".into(),
            port: 443,
            password: "password".into(),
            sni: Some("trojan.example.com".into()),
            network: Some("tcp".into()),
        };
        let config = ConfigGenerator::generate(&trojan, 20003).unwrap();
        assert_eq!(config.tag, "trojan-9.10.11.12-443");

        let ob = &config.outbound_json;
        assert_eq!(ob["protocol"], "trojan");
        assert_eq!(ob["settings"]["servers"][0]["address"], "9.10.11.12");
        assert_eq!(ob["settings"]["servers"][0]["password"], "password");
        assert_eq!(ob["streamSettings"]["network"], "tcp");
        assert_eq!(ob["streamSettings"]["security"], "tls");
        assert_eq!(
            ob["streamSettings"]["tlsSettings"]["serverName"],
            "trojan.example.com"
        );
    }

    #[test]
    fn test_generate_trojan_no_sni() {
        let trojan = SubscriptionProxy::Trojan {
            host: "9.10.11.12".into(),
            port: 443,
            password: "password".into(),
            sni: None,
            network: None,
        };
        let config = ConfigGenerator::generate(&trojan, 20004).unwrap();
        let ob = &config.outbound_json;
        // No SNI => no TLS section
        assert_eq!(ob["streamSettings"]["network"], "tcp");
        assert!(ob["streamSettings"].get("security").is_none());
    }

    #[test]
    fn test_generate_basic_returns_none() {
        let basic = SubscriptionProxy::Basic {
            host: "1.2.3.4".into(),
            port: 1080,
            protocol: Protocol::Socks5,
            username: None,
            password: None,
        };
        assert!(ConfigGenerator::generate(&basic, 20000).is_none());
    }

    #[test]
    fn test_generate_unknown_returns_none() {
        let unknown = SubscriptionProxy::Unknown {
            raw_config: "garbage".into(),
        };
        assert!(ConfigGenerator::generate(&unknown, 20000).is_none());
    }

    #[test]
    fn test_bootstrap_config_valid_json() {
        let json_str = ConfigGenerator::generate_bootstrap_config(10085);
        let parsed: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["api"]["tag"], "api");
        assert_eq!(parsed["inbounds"][0]["port"], 10085);
        assert_eq!(parsed["outbounds"][0]["protocol"], "freedom");
    }

    #[test]
    fn test_inbound_json_structure() {
        let ib = generate_inbound_json("in-ss-1.2.3.4-8388", 20000);
        assert_eq!(ib["tag"], "in-ss-1.2.3.4-8388");
        assert_eq!(ib["protocol"], "socks");
        assert_eq!(ib["port"], 20000);
        assert_eq!(ib["listen"], "127.0.0.1");
        assert_eq!(ib["settings"]["auth"], "noauth");
    }

    #[test]
    fn test_routing_rule_json_structure() {
        let rr = generate_routing_rule_json("in-tag", "out-tag");
        assert_eq!(rr["type"], "field");
        assert_eq!(rr["inboundTag"], json!(["in-tag"]));
        assert_eq!(rr["outboundTag"], "out-tag");
        // ruleTag must be present so the rule can later be removed by tag.
        assert_eq!(rr["ruleTag"], "rule-in-tag");
        assert_eq!(routing_rule_tag("in-tag"), "rule-in-tag");
    }

    #[test]
    fn test_bootstrap_config_enables_routing_service() {
        let config = ConfigGenerator::generate_bootstrap_config(10085);
        let value: Value = serde_json::from_str(&config).unwrap();
        let services = value["api"]["services"].as_array().unwrap();
        // RoutingService is required for runtime `adrules`/`rmrules`; without it
        // per-node routing rules cannot be installed and encrypted nodes egress
        // through the bootstrap `direct` outbound.
        assert!(services.iter().any(|s| s == "HandlerService"));
        assert!(services.iter().any(|s| s == "RoutingService"));
    }

    #[test]
    fn test_shadowsocks_cipher_supported_whitelist() {
        // AEAD + 2022 + none are accepted (case-insensitive).
        for ok in [
            "aes-256-gcm",
            "AES-128-GCM",
            "chacha20-ietf-poly1305",
            "chacha20-poly1305",
            "2022-blake3-aes-256-gcm",
            "none",
            "plain",
        ] {
            assert!(shadowsocks_cipher_supported(ok), "{ok} should be supported");
        }
        // Legacy stream ciphers xray-core rejects.
        for bad in ["aes-256-cfb", "aes-128-ctr", "rc4-md5", "salsa20", ""] {
            assert!(
                !shadowsocks_cipher_supported(bad),
                "{bad} should be unsupported"
            );
        }
    }

    #[test]
    fn test_is_xray_activatable() {
        let good_ss = SubscriptionProxy::Shadowsocks {
            host: "1.2.3.4".into(),
            port: 8388,
            method: "aes-256-gcm".into(),
            password: "p".into(),
            plugin: None,
            plugin_opts: None,
        };
        let legacy_ss = SubscriptionProxy::Shadowsocks {
            host: "1.2.3.4".into(),
            port: 8388,
            method: "aes-256-cfb".into(),
            password: "p".into(),
            plugin: None,
            plugin_opts: None,
        };
        assert!(is_xray_activatable(&good_ss));
        assert!(!is_xray_activatable(&legacy_ss));
        assert!(!is_xray_activatable(&SubscriptionProxy::Unknown {
            raw_config: "x".into()
        }));
    }

    #[test]
    fn test_http_transport_mapped_to_xhttp() {
        // xray-core 1.8.24+ removed "http" transport; it must be mapped to "xhttp".
        let trojan_http = SubscriptionProxy::Trojan {
            host: "1.2.3.4".into(),
            port: 443,
            password: "pass".into(),
            sni: Some("example.com".into()),
            network: Some("http".into()),
        };
        let config = ConfigGenerator::generate(&trojan_http, 20010).unwrap();
        let ob = &config.outbound_json;
        assert_eq!(ob["streamSettings"]["network"], "xhttp");

        // "h2" should also map to "xhttp"
        let trojan_h2 = SubscriptionProxy::Trojan {
            host: "1.2.3.4".into(),
            port: 443,
            password: "pass".into(),
            sni: Some("example.com".into()),
            network: Some("h2".into()),
        };
        let config_h2 = ConfigGenerator::generate(&trojan_h2, 20011).unwrap();
        assert_eq!(
            config_h2.outbound_json["streamSettings"]["network"],
            "xhttp"
        );
    }

    #[test]
    fn test_build_stream_settings_maps_http_h2_to_xhttp() {
        let none_path = None;
        let none_host = None;
        let none_service = None;
        let none_security = None;
        let none_sni = None;
        let none_fp = None;
        let none_pk = None;
        let none_sid = None;
        let none_spider = None;

        let stream_http = build_stream_settings(StreamSettingsInput {
            network: "http",
            path: &none_path,
            host_header: &none_host,
            service_name: &none_service,
            security: &none_security,
            sni: &none_sni,
            fingerprint: &none_fp,
            public_key: &none_pk,
            short_id: &none_sid,
            spider_x: &none_spider,
        });
        assert_eq!(stream_http["network"], "xhttp");

        let stream_h2 = build_stream_settings(StreamSettingsInput {
            network: "h2",
            path: &none_path,
            host_header: &none_host,
            service_name: &none_service,
            security: &none_security,
            sni: &none_sni,
            fingerprint: &none_fp,
            public_key: &none_pk,
            short_id: &none_sid,
            spider_x: &none_spider,
        });
        assert_eq!(stream_h2["network"], "xhttp");
    }
}
