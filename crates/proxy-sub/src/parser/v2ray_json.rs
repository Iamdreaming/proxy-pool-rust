//! V2Ray JSON subscription parser.
//!
//! Extracts the `outbounds` array from V2Ray/Xray JSON config.
//! Supported protocols: socks, http, vmess, shadowsocks, trojan, vless.
//! Unsupported protocols map to `Unknown`.

use crate::models::SubscriptionProxy;
use crate::parser::Parser;
use proxy_core::models::Protocol;
use serde_json::Value;

/// V2Ray JSON format parser.
pub struct V2rayJsonParser;

impl Parser for V2rayJsonParser {
    fn name(&self) -> &str {
        "V2Ray JSON"
    }

    fn detect(&self, content: &str) -> bool {
        let trimmed = content.trim();
        // Fast reject: must start with { or [
        if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
            return false;
        }
        // Parse as JSON and look for `outbounds` key
        match serde_json::from_str::<Value>(trimmed) {
            Ok(Value::Object(map)) => map.contains_key("outbounds"),
            Ok(Value::Array(_)) => {
                // Array of outbound objects — check first element for protocol
                true
            }
            _ => false,
        }
    }

    fn parse(&self, content: &str) -> Vec<SubscriptionProxy> {
        let trimmed = content.trim();
        let root: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("V2Ray JSON: parse error: {e}");
                return Vec::new();
            }
        };

        let outbounds = match extract_outbounds(&root) {
            Some(arr) => arr,
            None => {
                tracing::warn!("V2Ray JSON: no outbounds array found");
                return Vec::new();
            }
        };

        outbounds.iter().filter_map(parse_outbound).collect()
    }
}

/// Extract the outbounds array from the JSON root.
/// Handles both `{ "outbounds": [...] }` and a bare `[...]` array.
fn extract_outbounds(root: &Value) -> Option<&Vec<Value>> {
    match root {
        Value::Object(map) => map.get("outbounds").and_then(|v| v.as_array()),
        Value::Array(arr) => Some(arr),
        _ => None,
    }
}

/// Parse a single outbound entry into a `SubscriptionProxy`.
fn parse_outbound(ob: &Value) -> Option<SubscriptionProxy> {
    let protocol = ob
        .get("protocol")
        .and_then(|v| v.as_str())?
        .to_ascii_lowercase();

    // Skip internal outbounds like "freedom", "blackhole", "dns"
    if matches!(protocol.as_str(), "freedom" | "blackhole" | "dns") {
        return None;
    }

    let settings = ob.get("settings").cloned().unwrap_or(Value::Null);
    let stream_settings = ob.get("streamSettings");

    match protocol.as_str() {
        "socks" => parse_socks(&settings),
        "http" => parse_http(&settings),
        "vmess" => parse_vmess(&settings, stream_settings),
        "shadowsocks" => parse_shadowsocks(&settings),
        "trojan" => parse_trojan(&settings, stream_settings),
        "vless" => parse_vless(&settings, stream_settings),
        _ => Some(SubscriptionProxy::Unknown {
            raw_config: format!(
                "{}: {}",
                protocol,
                ob.get("tag").and_then(|v| v.as_str()).unwrap_or("unknown")
            ),
        }),
    }
}

/// Parse a socks outbound → `SubscriptionProxy::Basic` with `Protocol::Socks5`.
fn parse_socks(settings: &Value) -> Option<SubscriptionProxy> {
    let server = first_server(settings)?;
    let host = server.get("address").and_then(|v| v.as_str())?.to_string();
    let port = server.get("port").and_then(|v| v.as_u64())? as u16;
    let (username, password) = server_user_pass(server);

    Some(SubscriptionProxy::Basic {
        host,
        port,
        protocol: Protocol::Socks5,
        username,
        password,
    })
}

/// Parse a VLESS outbound into `SubscriptionProxy::Vless`.
fn parse_vless(settings: &Value, stream_settings: Option<&Value>) -> Option<SubscriptionProxy> {
    let (host, port, user) = if let Some(vnext) = settings
        .get("vnext")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_object())
    {
        let host = vnext.get("address").and_then(|v| v.as_str())?.to_string();
        let port = value_as_u16(vnext.get("port")?)?;
        let user = vnext
            .get("users")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_object());
        (host, port, user)
    } else {
        let host = settings
            .get("address")
            .and_then(|v| v.as_str())?
            .to_string();
        let port = value_as_u16(settings.get("port")?)?;
        (host, port, settings.as_object())
    };

    let uuid = user
        .and_then(|u| u.get("id"))
        .or_else(|| settings.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let encryption = user
        .and_then(|u| u.get("encryption"))
        .or_else(|| settings.get("encryption"))
        .and_then(|v| v.as_str())
        .unwrap_or("none")
        .to_string();
    let flow = user
        .and_then(|u| u.get("flow"))
        .or_else(|| settings.get("flow"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);

    let (network, path_or_service, host_header, tls_sni) = extract_stream_settings(stream_settings);
    let (path, service_name) = if network == "grpc" {
        (None, path_or_service)
    } else {
        (
            path_or_service,
            stream_string(stream_settings, "grpcSettings", "serviceName"),
        )
    };

    Some(SubscriptionProxy::Vless {
        host,
        port,
        uuid,
        encryption,
        flow,
        network,
        security: stream_security(stream_settings),
        sni: stream_server_name(stream_settings).or(tls_sni),
        host_header,
        path,
        service_name,
        fingerprint: stream_string(stream_settings, "tlsSettings", "fingerprint")
            .or_else(|| stream_string(stream_settings, "realitySettings", "fingerprint")),
        public_key: stream_string(stream_settings, "realitySettings", "publicKey")
            .or_else(|| stream_string(stream_settings, "realitySettings", "password")),
        short_id: stream_string(stream_settings, "realitySettings", "shortId"),
        spider_x: stream_string(stream_settings, "realitySettings", "spiderX"),
    })
}

/// Parse an http outbound → `SubscriptionProxy::Basic` with `Protocol::Http`.
fn parse_http(settings: &Value) -> Option<SubscriptionProxy> {
    let server = first_server(settings)?;
    let host = server.get("address").and_then(|v| v.as_str())?.to_string();
    let port = server.get("port").and_then(|v| v.as_u64())? as u16;
    let (username, password) = server_user_pass(server);

    Some(SubscriptionProxy::Basic {
        host,
        port,
        protocol: Protocol::Http,
        username,
        password,
    })
}

/// Extract the first user's `user`/`pass` credentials from a socks/http server.
fn server_user_pass(server: &serde_json::Map<String, Value>) -> (Option<String>, Option<String>) {
    let user = server
        .get("users")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first());
    let get = |key: &str| -> Option<String> {
        user.and_then(|u| u.get(key))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    (get("user"), get("pass"))
}

/// Parse a vmess outbound → `SubscriptionProxy::Vmess`.
fn parse_vmess(settings: &Value, stream_settings: Option<&Value>) -> Option<SubscriptionProxy> {
    let vnext = settings.get("vnext")?.as_array()?.first()?.as_object()?;

    let host = vnext.get("address").and_then(|v| v.as_str())?.to_string();
    let port = vnext.get("port").and_then(|v| v.as_u64())? as u16;

    // Extract first user
    let user = vnext
        .get("users")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_object());

    let (uuid, alter_id, security) = match user {
        Some(u) => {
            let id = u
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let aid = u.get("alterId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let sec = u
                .get("security")
                .and_then(|v| v.as_str())
                .unwrap_or("auto")
                .to_string();
            (id, aid, sec)
        }
        None => (String::new(), 0, "auto".to_string()),
    };

    let (network, path, host_header, sni) = extract_stream_settings(stream_settings);

    Some(SubscriptionProxy::Vmess {
        host,
        port,
        uuid,
        alter_id,
        security,
        network,
        path,
        host_header,
        sni,
    })
}

/// Parse a shadowsocks outbound → `SubscriptionProxy::Shadowsocks`.
fn parse_shadowsocks(settings: &Value) -> Option<SubscriptionProxy> {
    let server = first_server(settings)?;
    let host = server.get("address").and_then(|v| v.as_str())?.to_string();
    let port = server.get("port").and_then(|v| v.as_u64())? as u16;
    let method = server
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let password = server
        .get("password")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Some(SubscriptionProxy::Shadowsocks {
        host,
        port,
        method,
        password,
        plugin: None,
        plugin_opts: None,
    })
}

/// Parse a trojan outbound → `SubscriptionProxy::Trojan`.
fn parse_trojan(settings: &Value, stream_settings: Option<&Value>) -> Option<SubscriptionProxy> {
    let server = first_server(settings)?;
    let host = server.get("address").and_then(|v| v.as_str())?.to_string();
    let port = server.get("port").and_then(|v| v.as_u64())? as u16;
    let password = server
        .get("password")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let (network, _, _, sni) = extract_stream_settings(stream_settings);

    Some(SubscriptionProxy::Trojan {
        host,
        port,
        password,
        sni,
        network: if network == "tcp" {
            None
        } else {
            Some(network)
        },
    })
}

/// Get the first server object from `settings.servers` array.
fn first_server(settings: &Value) -> Option<&serde_json::Map<String, Value>> {
    settings.get("servers")?.as_array()?.first()?.as_object()
}

/// Extract stream settings: (network, path, host_header, sni).
fn extract_stream_settings(
    ss: Option<&Value>,
) -> (String, Option<String>, Option<String>, Option<String>) {
    let ss = match ss {
        Some(v) => v,
        None => return ("tcp".to_string(), None, None, None),
    };

    let network = ss
        .get("network")
        .and_then(|v| v.as_str())
        .unwrap_or("tcp")
        .to_string();

    let path = match network.as_str() {
        "ws" => ss
            .get("wsSettings")
            .and_then(|v| v.get("path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "grpc" => ss
            .get("grpcSettings")
            .and_then(|v| v.get("serviceName"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        _ => None,
    };

    let host_header = ss
        .get("wsSettings")
        .and_then(|v| v.get("headers"))
        .and_then(|v| v.get("Host"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let sni = ss
        .get("tlsSettings")
        .and_then(|v| v.get("serverName"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    (network, path, host_header, sni)
}

fn value_as_u16(value: &Value) -> Option<u16> {
    value
        .as_u64()
        .and_then(|n| u16::try_from(n).ok())
        .or_else(|| value.as_str().and_then(|s| s.parse::<u16>().ok()))
}

fn stream_security(ss: Option<&Value>) -> Option<String> {
    ss.and_then(|v| v.get("security"))
        .and_then(|v| v.as_str())
        .filter(|value| !value.is_empty() && *value != "none")
        .map(ToString::to_string)
}

fn stream_server_name(ss: Option<&Value>) -> Option<String> {
    stream_string(ss, "tlsSettings", "serverName")
        .or_else(|| stream_string(ss, "realitySettings", "serverName"))
}

fn stream_string(ss: Option<&Value>, section: &str, key: &str) -> Option<String> {
    ss.and_then(|v| v.get(section))
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_str())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SubscriptionProxy;

    const FIXTURE: &str = include_str!("../../tests/fixtures/v2ray_sample.json");

    #[test]
    fn test_detect_valid_json_with_outbounds() {
        let parser = V2rayJsonParser;
        let json = r#"{"outbounds": [{"protocol": "socks"}]}"#;
        assert!(parser.detect(json));
    }

    #[test]
    fn test_detect_valid_json_array() {
        let parser = V2rayJsonParser;
        let json = r#"[{"protocol": "socks"}]"#;
        assert!(parser.detect(json));
    }

    #[test]
    fn test_detect_not_json() {
        let parser = V2rayJsonParser;
        assert!(!parser.detect("just some random text"));
        assert!(!parser.detect("proxies:\n  - name: test"));
    }

    #[test]
    fn test_detect_json_without_outbounds() {
        let parser = V2rayJsonParser;
        assert!(!parser.detect(r#"{"inbounds": []}"#));
    }

    #[test]
    fn test_detect_empty() {
        let parser = V2rayJsonParser;
        assert!(!parser.detect(""));
        assert!(!parser.detect("   "));
    }

    #[test]
    fn test_parse_socks() {
        let parser = V2rayJsonParser;
        let json = r#"{"outbounds": [{"protocol": "socks", "settings": {"servers": [{"address": "10.0.0.1", "port": 1080}]}, "tag": "socks-proxy"}]}"#;
        let proxies = parser.parse(json);
        assert_eq!(proxies.len(), 1);
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
        } else {
            panic!("Expected Basic, got {:?}", proxies[0]);
        }
    }

    #[test]
    fn test_parse_http() {
        let parser = V2rayJsonParser;
        let json = r#"{"outbounds": [{"protocol": "http", "settings": {"servers": [{"address": "10.0.0.2", "port": 8080}]}, "tag": "http-proxy"}]}"#;
        let proxies = parser.parse(json);
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Basic {
            host,
            port,
            protocol,
            ..
        } = &proxies[0]
        {
            assert_eq!(host, "10.0.0.2");
            assert_eq!(*port, 8080);
            assert_eq!(*protocol, Protocol::Http);
        } else {
            panic!("Expected Basic, got {:?}", proxies[0]);
        }
    }

    #[test]
    fn test_parse_vmess() {
        let parser = V2rayJsonParser;
        let json = r#"{"outbounds": [{"protocol": "vmess", "settings": {"vnext": [{"address": "10.0.0.3", "port": 443, "users": [{"id": "a3482e88-686a-4a58-8126-99c9df64b7bf", "alterId": 0, "security": "auto"}]}]}, "streamSettings": {"network": "ws", "wsSettings": {"path": "/v2", "headers": {"Host": "vmess.example.com"}}, "security": "tls", "tlsSettings": {"serverName": "sni.example.com"}}, "tag": "vmess-proxy"}]}"#;
        let proxies = parser.parse(json);
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Vmess {
            host,
            port,
            uuid,
            alter_id,
            security,
            network,
            path,
            host_header,
            sni,
        } = &proxies[0]
        {
            assert_eq!(host, "10.0.0.3");
            assert_eq!(*port, 443);
            assert_eq!(uuid, "a3482e88-686a-4a58-8126-99c9df64b7bf");
            assert_eq!(*alter_id, 0);
            assert_eq!(security, "auto");
            assert_eq!(network, "ws");
            assert_eq!(path.as_deref(), Some("/v2"));
            assert_eq!(host_header.as_deref(), Some("vmess.example.com"));
            assert_eq!(sni.as_deref(), Some("sni.example.com"));
        } else {
            panic!("Expected Vmess, got {:?}", proxies[0]);
        }
    }

    #[test]
    fn test_parse_vmess_no_stream_settings() {
        let parser = V2rayJsonParser;
        let json = r#"{"outbounds": [{"protocol": "vmess", "settings": {"vnext": [{"address": "10.0.0.4", "port": 443, "users": [{"id": "test-uuid", "alterId": 0, "security": "aes-128-gcm"}]}]}, "tag": "vmess-tcp"}]}"#;
        let proxies = parser.parse(json);
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Vmess {
            network,
            path,
            host_header,
            sni,
            ..
        } = &proxies[0]
        {
            assert_eq!(network, "tcp");
            assert!(path.is_none());
            assert!(host_header.is_none());
            assert!(sni.is_none());
        } else {
            panic!("Expected Vmess, got {:?}", proxies[0]);
        }
    }

    #[test]
    fn test_parse_vmess_grpc() {
        let parser = V2rayJsonParser;
        let json = r#"{"outbounds": [{"protocol": "vmess", "settings": {"vnext": [{"address": "10.0.0.5", "port": 443, "users": [{"id": "grpc-uuid", "alterId": 0, "security": "auto"}]}]}, "streamSettings": {"network": "grpc", "grpcSettings": {"serviceName": "grpc-service"}}, "tag": "vmess-grpc"}]}"#;
        let proxies = parser.parse(json);
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Vmess { network, path, .. } = &proxies[0] {
            assert_eq!(network, "grpc");
            assert_eq!(path.as_deref(), Some("grpc-service"));
        } else {
            panic!("Expected Vmess, got {:?}", proxies[0]);
        }
    }

    #[test]
    fn test_parse_shadowsocks() {
        let parser = V2rayJsonParser;
        let json = r#"{"outbounds": [{"protocol": "shadowsocks", "settings": {"servers": [{"address": "10.0.0.6", "port": 8388, "method": "aes-256-gcm", "password": "ss-pass"}]}, "tag": "ss-proxy"}]}"#;
        let proxies = parser.parse(json);
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Shadowsocks {
            host,
            port,
            method,
            password,
            plugin,
            plugin_opts,
        } = &proxies[0]
        {
            assert_eq!(host, "10.0.0.6");
            assert_eq!(*port, 8388);
            assert_eq!(method, "aes-256-gcm");
            assert_eq!(password, "ss-pass");
            assert!(plugin.is_none());
            assert!(plugin_opts.is_none());
        } else {
            panic!("Expected Shadowsocks, got {:?}", proxies[0]);
        }
    }

    #[test]
    fn test_parse_trojan() {
        let parser = V2rayJsonParser;
        let json = r#"{"outbounds": [{"protocol": "trojan", "settings": {"servers": [{"address": "10.0.0.7", "port": 443, "password": "trojan-pass"}]}, "streamSettings": {"network": "tcp", "security": "tls", "tlsSettings": {"serverName": "trojan.example.com"}}, "tag": "trojan-proxy"}]}"#;
        let proxies = parser.parse(json);
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Trojan {
            host,
            port,
            password,
            sni,
            network,
        } = &proxies[0]
        {
            assert_eq!(host, "10.0.0.7");
            assert_eq!(*port, 443);
            assert_eq!(password, "trojan-pass");
            assert_eq!(sni.as_deref(), Some("trojan.example.com"));
            // network=tcp maps to None
            assert!(network.is_none());
        } else {
            panic!("Expected Trojan, got {:?}", proxies[0]);
        }
    }

    #[test]
    fn test_parse_trojan_ws_network() {
        let parser = V2rayJsonParser;
        let json = r#"{"outbounds": [{"protocol": "trojan", "settings": {"servers": [{"address": "10.0.0.8", "port": 443, "password": "ws-pass"}]}, "streamSettings": {"network": "ws", "security": "tls", "tlsSettings": {"serverName": "ws.example.com"}, "wsSettings": {"path": "/ws"}}, "tag": "trojan-ws"}]}"#;
        let proxies = parser.parse(json);
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Trojan {
            password,
            sni,
            network,
            ..
        } = &proxies[0]
        {
            assert_eq!(password, "ws-pass");
            assert_eq!(sni.as_deref(), Some("ws.example.com"));
            assert_eq!(network.as_deref(), Some("ws"));
        } else {
            panic!("Expected Trojan, got {:?}", proxies[0]);
        }
    }

    #[test]
    fn test_parse_vless() {
        let parser = V2rayJsonParser;
        let json = r#"{"outbounds": [{"protocol": "vless", "settings": {"vnext": [{"address": "10.0.0.9", "port": 443, "users": [{"id": "550e8400-e29b-41d4-a716-446655440000", "encryption": "none", "flow": "xtls-rprx-vision"}]}]}, "streamSettings": {"network": "ws", "security": "tls", "tlsSettings": {"serverName": "vless.example.com", "fingerprint": "chrome"}, "wsSettings": {"path": "/vless", "headers": {"Host": "cdn.example.com"}}}, "tag": "vless-proxy"}]}"#;
        let proxies = parser.parse(json);
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
            host_header,
            path,
            fingerprint,
            ..
        } = &proxies[0]
        {
            assert_eq!(host, "10.0.0.9");
            assert_eq!(*port, 443);
            assert_eq!(uuid, "550e8400-e29b-41d4-a716-446655440000");
            assert_eq!(encryption, "none");
            assert_eq!(flow.as_deref(), Some("xtls-rprx-vision"));
            assert_eq!(network, "ws");
            assert_eq!(security.as_deref(), Some("tls"));
            assert_eq!(sni.as_deref(), Some("vless.example.com"));
            assert_eq!(host_header.as_deref(), Some("cdn.example.com"));
            assert_eq!(path.as_deref(), Some("/vless"));
            assert_eq!(fingerprint.as_deref(), Some("chrome"));
        } else {
            panic!("Expected Vless, got {:?}", proxies[0]);
        }
    }

    #[test]
    fn test_parse_vless_reality_direct_settings() {
        let parser = V2rayJsonParser;
        let json = r#"{"outbounds": [{"protocol": "vless", "settings": {"address": "reality.example.com", "port": "443", "id": "uid", "encryption": "none"}, "streamSettings": {"network": "tcp", "security": "reality", "realitySettings": {"serverName": "www.microsoft.com", "fingerprint": "chrome", "publicKey": "pub-key", "shortId": "abcd", "spiderX": "/"}}}]}"#;
        let proxies = parser.parse(json);
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Vless {
            host,
            security,
            sni,
            public_key,
            short_id,
            spider_x,
            ..
        } = &proxies[0]
        {
            assert_eq!(host, "reality.example.com");
            assert_eq!(security.as_deref(), Some("reality"));
            assert_eq!(sni.as_deref(), Some("www.microsoft.com"));
            assert_eq!(public_key.as_deref(), Some("pub-key"));
            assert_eq!(short_id.as_deref(), Some("abcd"));
            assert_eq!(spider_x.as_deref(), Some("/"));
        } else {
            panic!("Expected Vless, got {:?}", proxies[0]);
        }
    }

    #[test]
    fn test_parse_unknown_protocol() {
        let parser = V2rayJsonParser;
        let json =
            r#"{"outbounds": [{"protocol": "wireguard", "settings": {}, "tag": "wg-proxy"}]}"#;
        let proxies = parser.parse(json);
        assert_eq!(proxies.len(), 1);
        assert!(matches!(&proxies[0], SubscriptionProxy::Unknown { .. }));
    }

    #[test]
    fn test_parse_skips_freedom_blackhole() {
        let parser = V2rayJsonParser;
        let json = r#"{"outbounds": [
            {"protocol": "freedom", "tag": "direct"},
            {"protocol": "blackhole", "tag": "block"},
            {"protocol": "socks", "settings": {"servers": [{"address": "10.0.0.1", "port": 1080}]}, "tag": "socks-proxy"}
        ]}"#;
        let proxies = parser.parse(json);
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Basic { protocol, .. } = &proxies[0] {
            assert_eq!(*protocol, Protocol::Socks5);
        }
    }

    #[test]
    fn test_parse_no_outbounds_key() {
        let parser = V2rayJsonParser;
        let json = r#"{"inbounds": []}"#;
        let proxies = parser.parse(json);
        assert!(proxies.is_empty());
    }

    #[test]
    fn test_parse_invalid_json() {
        let parser = V2rayJsonParser;
        let proxies = parser.parse("{invalid json}");
        assert!(proxies.is_empty());
    }

    #[test]
    fn test_parse_empty_outbounds() {
        let parser = V2rayJsonParser;
        let json = r#"{"outbounds": []}"#;
        let proxies = parser.parse(json);
        assert!(proxies.is_empty());
    }

    // -- Fixture test --
    #[test]
    fn test_fixture_v2ray_sample() {
        let parser = V2rayJsonParser;
        assert!(parser.detect(FIXTURE));
        let proxies = parser.parse(FIXTURE);
        assert_eq!(proxies.len(), 4);

        // First: socks5 → Basic
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
        } else {
            panic!("Expected Basic (socks5), got {:?}", proxies[0]);
        }

        // Second: vmess → Vmess
        if let SubscriptionProxy::Vmess {
            host,
            port,
            uuid,
            network,
            path,
            host_header,
            sni,
            ..
        } = &proxies[1]
        {
            assert_eq!(host, "10.0.0.3");
            assert_eq!(*port, 443);
            assert_eq!(uuid, "a3482e88-686a-4a58-8126-99c9df64b7bf");
            assert_eq!(network, "ws");
            assert_eq!(path.as_deref(), Some("/v2"));
            assert_eq!(host_header.as_deref(), Some("vmess.example.com"));
            assert_eq!(sni.as_deref(), Some("sni.example.com"));
        } else {
            panic!("Expected Vmess, got {:?}", proxies[1]);
        }

        // Third: trojan → Trojan
        if let SubscriptionProxy::Trojan {
            host,
            port,
            password,
            sni,
            ..
        } = &proxies[2]
        {
            assert_eq!(host, "10.0.0.7");
            assert_eq!(*port, 443);
            assert_eq!(password, "trojan-pass");
            assert_eq!(sni.as_deref(), Some("trojan.example.com"));
        } else {
            panic!("Expected Trojan, got {:?}", proxies[2]);
        }

        // Fourth: shadowsocks → Shadowsocks
        if let SubscriptionProxy::Shadowsocks {
            host,
            port,
            method,
            password,
            ..
        } = &proxies[3]
        {
            assert_eq!(host, "10.0.0.6");
            assert_eq!(*port, 8388);
            assert_eq!(method, "aes-256-gcm");
            assert_eq!(password, "ss-pass");
        } else {
            panic!("Expected Shadowsocks, got {:?}", proxies[3]);
        }
    }

    // -- extract_stream_settings unit tests --
    #[test]
    fn test_extract_stream_settings_none() {
        let (network, path, host_header, sni) = extract_stream_settings(None);
        assert_eq!(network, "tcp");
        assert!(path.is_none());
        assert!(host_header.is_none());
        assert!(sni.is_none());
    }

    #[test]
    fn test_extract_stream_settings_ws() {
        let ss = serde_json::json!({
            "network": "ws",
            "wsSettings": {
                "path": "/ws-path",
                "headers": { "Host": "ws-host.example.com" }
            },
            "security": "tls",
            "tlsSettings": { "serverName": "sni.example.com" }
        });
        let (network, path, host_header, sni) = extract_stream_settings(Some(&ss));
        assert_eq!(network, "ws");
        assert_eq!(path.as_deref(), Some("/ws-path"));
        assert_eq!(host_header.as_deref(), Some("ws-host.example.com"));
        assert_eq!(sni.as_deref(), Some("sni.example.com"));
    }

    #[test]
    fn test_extract_stream_settings_grpc() {
        let ss = serde_json::json!({
            "network": "grpc",
            "grpcSettings": { "serviceName": "my-grpc-service" }
        });
        let (network, path, host_header, sni) = extract_stream_settings(Some(&ss));
        assert_eq!(network, "grpc");
        assert_eq!(path.as_deref(), Some("my-grpc-service"));
        assert!(host_header.is_none());
        assert!(sni.is_none());
    }

    #[test]
    fn test_extract_stream_settings_tcp_default() {
        let ss = serde_json::json!({
            "network": "tcp",
            "security": "tls",
            "tlsSettings": { "serverName": "tcp-sni.example.com" }
        });
        let (network, path, host_header, sni) = extract_stream_settings(Some(&ss));
        assert_eq!(network, "tcp");
        assert!(path.is_none());
        assert!(host_header.is_none());
        assert_eq!(sni.as_deref(), Some("tcp-sni.example.com"));
    }

    #[test]
    fn test_extract_stream_settings_no_network() {
        let ss = serde_json::json!({
            "security": "none"
        });
        let (network, path, host_header, sni) = extract_stream_settings(Some(&ss));
        assert_eq!(network, "tcp");
        assert!(path.is_none());
        assert!(host_header.is_none());
        assert!(sni.is_none());
    }
}
