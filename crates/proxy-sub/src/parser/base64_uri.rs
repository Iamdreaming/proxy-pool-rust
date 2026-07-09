//! Base64 URI subscription parser.
//!
//! Handles the common subscription format where content is either:
//! 1. One big base64 blob that decodes to newline-separated proxy URIs
//! 2. Individual lines, each a base64-encoded or plain proxy URI
//!
//! Supported URI schemes: `ss://`, `vmess://`, `trojan://`, `vless://`,
//! `socks5://`, `http://`.

use crate::models::SubscriptionProxy;
use crate::parser::Parser;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE};
use proxy_core::models::Protocol;

/// Base64 URI format parser.
pub struct Base64UriParser;

/// Known URI scheme prefixes.
const SCHEMES: &[&str] = &[
    "ss://",
    "vmess://",
    "trojan://",
    "vless://",
    "socks5://",
    "http://",
];

impl Parser for Base64UriParser {
    fn name(&self) -> &str {
        "Base64 URI"
    }

    fn detect(&self, content: &str) -> bool {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return false;
        }
        // Heuristic 1: try decoding entire content as base64, check for ://
        if let Ok(decoded) = decode_base64(trimmed)
            && decoded.contains("://")
        {
            return true;
        }
        // Heuristic 2: any line starts with a known scheme
        for line in trimmed.lines() {
            let line = line.trim();
            if SCHEMES.iter().any(|s| line.starts_with(s)) {
                return true;
            }
            // Line might be base64-encoded URI
            if let Ok(decoded_line) = decode_base64(line)
                && SCHEMES.iter().any(|s| decoded_line.starts_with(s))
            {
                return true;
            }
        }
        false
    }

    fn parse(&self, content: &str) -> Vec<SubscriptionProxy> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }

        // Try full-blob decode first
        if let Ok(decoded) = decode_base64(trimmed)
            && decoded.contains("://")
        {
            return decode_lines(&decoded);
        }

        // Fall back to line-by-line processing
        decode_lines(trimmed)
    }
}

/// Decode lines of text (possibly base64-encoded per line) into proxy nodes.
fn decode_lines(text: &str) -> Vec<SubscriptionProxy> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            Some(parse_line(line))
        })
        .collect()
}

/// Parse a single line: either a plain URI or base64-encoded URI.
fn parse_line(line: &str) -> SubscriptionProxy {
    // If already a known scheme, parse directly
    if SCHEMES.iter().any(|s| line.starts_with(s)) {
        return parse_uri(line);
    }
    // Try base64 decode
    if let Ok(decoded) = decode_base64(line)
        && !decoded.is_empty()
    {
        return parse_uri(&decoded);
    }
    SubscriptionProxy::Unknown {
        raw_config: line.to_string(),
    }
}

/// Decode a base64 string, trying both STANDARD and URL_SAFE alphabets.
/// Handles missing padding by appending `=` as needed.
fn decode_base64(input: &str) -> Result<String, DecodeError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(DecodeError::InvalidLength(0));
    }

    // Pad to multiple of 4
    let padded = pad_base64(input);

    // Try STANDARD first, then URL_SAFE
    let bytes = STANDARD
        .decode(&padded)
        .or_else(|_| URL_SAFE.decode(&padded))
        .map_err(DecodeError::Base64)?;

    String::from_utf8(bytes).map_err(DecodeError::Utf8)
}

/// Wrapper error type for base64 decode failures.
#[derive(Debug)]
enum DecodeError {
    InvalidLength(usize),
    Base64(base64::DecodeError),
    Utf8(std::string::FromUtf8Error),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLength(len) => write!(f, "invalid length: {len}"),
            Self::Base64(e) => write!(f, "base64 decode error: {e}"),
            Self::Utf8(e) => write!(f, "utf8 decode error: {e}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Append `=` padding so the length becomes a multiple of 4.
fn pad_base64(s: &str) -> String {
    let remainder = s.len() % 4;
    if remainder == 0 {
        s.to_string()
    } else {
        let mut padded = s.to_string();
        for _ in 0..(4 - remainder) {
            padded.push('=');
        }
        padded
    }
}

/// Route a URI to its specific parser based on scheme.
fn parse_uri(uri: &str) -> SubscriptionProxy {
    if let Some(rest) = uri.strip_prefix("ss://") {
        parse_ss(rest)
    } else if let Some(rest) = uri.strip_prefix("vmess://") {
        parse_vmess(rest)
    } else if let Some(rest) = uri.strip_prefix("trojan://") {
        parse_trojan(rest)
    } else if let Some(rest) = uri.strip_prefix("vless://") {
        parse_vless(rest)
    } else if let Some(rest) = uri.strip_prefix("socks5://") {
        parse_basic(rest, Protocol::Socks5)
    } else if let Some(rest) = uri.strip_prefix("http://") {
        parse_basic(rest, Protocol::Http)
    } else {
        SubscriptionProxy::Unknown {
            raw_config: uri.to_string(),
        }
    }
}

/// Parse `socks5://host:port` or `http://host:port`.
fn parse_basic(rest: &str, protocol: Protocol) -> SubscriptionProxy {
    let (host, port) = match split_host_port(rest) {
        Some(pair) => pair,
        None => {
            tracing::warn!("Base64 URI: invalid basic URI: {rest}");
            return SubscriptionProxy::Unknown {
                raw_config: format!("{}://{rest}", protocol.scheme()),
            };
        }
    };
    SubscriptionProxy::Basic {
        host,
        port,
        protocol,
    }
}

/// Parse `ss://` URI.
///
/// Two forms:
/// 1. `ss://base64(method:password)@host:port?plugin=xxx#name`
/// 2. `ss://method:password@host:port?plugin=xxx#name`
fn parse_ss(rest: &str) -> SubscriptionProxy {
    // Strip fragment (#name)
    let rest = match rest.find('#') {
        Some(i) => &rest[..i],
        None => rest,
    };

    // Split userinfo@hostport
    let (user_info, hostport_and_query) = match rest.find('@') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => {
            tracing::warn!("Base64 URI: ss URI missing '@': {rest}");
            return SubscriptionProxy::Unknown {
                raw_config: format!("ss://{rest}"),
            };
        }
    };

    // Split host:port from query string
    let (hostport, query) = match hostport_and_query.find('?') {
        Some(i) => (&hostport_and_query[..i], &hostport_and_query[i + 1..]),
        None => (hostport_and_query, ""),
    };

    let (host, port) = match split_host_port(hostport) {
        Some(pair) => pair,
        None => {
            tracing::warn!("Base64 URI: ss invalid host:port: {hostport}");
            return SubscriptionProxy::Unknown {
                raw_config: format!("ss://{rest}"),
            };
        }
    };

    // Decode method:password — try base64 first, then plain
    let (method, password) = if let Ok(decoded) = decode_base64(user_info) {
        match decoded.split_once(':') {
            Some((m, p)) => (m.to_string(), p.to_string()),
            None => {
                tracing::warn!("Base64 URI: ss decoded userinfo has no ':': {decoded}");
                return SubscriptionProxy::Unknown {
                    raw_config: format!("ss://{rest}"),
                };
            }
        }
    } else {
        // Plain text method:password
        match user_info.split_once(':') {
            Some((m, p)) => (m.to_string(), p.to_string()),
            None => {
                tracing::warn!("Base64 URI: ss plain userinfo has no ':': {user_info}");
                return SubscriptionProxy::Unknown {
                    raw_config: format!("ss://{rest}"),
                };
            }
        }
    };

    // Parse query for plugin and plugin-opts
    let (plugin, plugin_opts) = parse_ss_query(query);

    SubscriptionProxy::Shadowsocks {
        host,
        port,
        method,
        password,
        plugin,
        plugin_opts,
    }
}

/// Parse SS query string for `plugin` and `plugin-opts`.
fn parse_ss_query(query: &str) -> (Option<String>, Option<String>) {
    let mut plugin = None;
    let mut plugin_opts = None;
    for pair in query.split('&') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        if let Some((key, value)) = pair.split_once('=') {
            match key {
                "plugin" => plugin = Some(value.to_string()),
                "plugin-opts" => plugin_opts = Some(value.to_string()),
                _ => {}
            }
        }
    }
    (plugin, plugin_opts)
}

/// Parse `vmess://base64_json` URI.
///
/// The base64-encoded JSON follows the V2Ray share link standard.
fn parse_vmess(rest: &str) -> SubscriptionProxy {
    let json_str = match decode_base64(rest) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Base64 URI: vmess base64 decode failed: {e}");
            return SubscriptionProxy::Unknown {
                raw_config: format!("vmess://{rest}"),
            };
        }
    };

    let v: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Base64 URI: vmess JSON parse failed: {e}");
            return SubscriptionProxy::Unknown {
                raw_config: format!("vmess://{rest}"),
            };
        }
    };

    let obj = match v.as_object() {
        Some(o) => o,
        None => {
            tracing::warn!("Base64 URI: vmess JSON is not an object");
            return SubscriptionProxy::Unknown {
                raw_config: format!("vmess://{rest}"),
            };
        }
    };

    let get_str = |key1: &str, key2: &str| -> String {
        obj.get(key1)
            .or_else(|| obj.get(key2))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };

    let get_str_opt = |key1: &str, key2: &str| -> Option<String> {
        let val = get_str(key1, key2);
        if val.is_empty() { None } else { Some(val) }
    };

    let host = get_str("add", "hnb");
    // vmess share links use either a string or a numeric port.
    let port: u16 = match obj.get("port").or_else(|| obj.get("pnt")).and_then(|v| {
        v.as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| v.as_u64().and_then(|n| u16::try_from(n).ok()))
    }) {
        Some(p) => p,
        None => {
            tracing::warn!("Base64 URI: vmess invalid or missing port");
            return SubscriptionProxy::Unknown {
                raw_config: format!("vmess://{rest}"),
            };
        }
    };
    let uuid = get_str("id", "uid");
    let alter_id: u32 = obj
        .get("aid")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| v.as_u64())
                .map(|n| n as u32)
        })
        .unwrap_or(0);
    let security = get_str("scy", "cipher");
    let security = if security.is_empty() {
        "auto".to_string()
    } else {
        security
    };
    let network = get_str("net", "net");
    let network = if network.is_empty() {
        "tcp".to_string()
    } else {
        network
    };
    let path = get_str_opt("path", "path");
    let host_header = get_str_opt("host", "host");
    let sni = get_str_opt("sni", "sni");

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
    }
}

/// Parse `trojan://password@host:port?sni=xxx&type=tcp` URI.
fn parse_trojan(rest: &str) -> SubscriptionProxy {
    // Strip fragment
    let rest = match rest.find('#') {
        Some(i) => &rest[..i],
        None => rest,
    };

    // Split password@hostport?query
    let (password, hostport_and_query) = match rest.find('@') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => {
            tracing::warn!("Base64 URI: trojan URI missing '@': {rest}");
            return SubscriptionProxy::Unknown {
                raw_config: format!("trojan://{rest}"),
            };
        }
    };

    let (hostport, query) = match hostport_and_query.find('?') {
        Some(i) => (&hostport_and_query[..i], &hostport_and_query[i + 1..]),
        None => (hostport_and_query, ""),
    };

    let (host, port) = match split_host_port(hostport) {
        Some(pair) => pair,
        None => {
            tracing::warn!("Base64 URI: trojan invalid host:port: {hostport}");
            return SubscriptionProxy::Unknown {
                raw_config: format!("trojan://{rest}"),
            };
        }
    };

    let (sni, network) = parse_trojan_query(query);

    SubscriptionProxy::Trojan {
        host,
        port,
        password: percent_decode(password),
        sni,
        network,
    }
}

/// Parse trojan query string for `sni` and `type`.
fn parse_trojan_query(query: &str) -> (Option<String>, Option<String>) {
    let mut sni = None;
    let mut network = None;
    for pair in query.split('&') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        if let Some((key, value)) = pair.split_once('=') {
            match key {
                "sni" => sni = Some(value.to_string()),
                "type" => network = Some(value.to_string()),
                _ => {}
            }
        }
    }
    (sni, network)
}

/// Parse `vless://uuid@host:port?...` URI.
fn parse_vless(rest: &str) -> SubscriptionProxy {
    let rest = match rest.find('#') {
        Some(i) => &rest[..i],
        None => rest,
    };

    let (uuid, hostport_and_query) = match rest.find('@') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => {
            tracing::warn!("Base64 URI: vless URI missing '@': {rest}");
            return SubscriptionProxy::Unknown {
                raw_config: format!("vless://{rest}"),
            };
        }
    };

    let (hostport, query) = match hostport_and_query.find('?') {
        Some(i) => (&hostport_and_query[..i], &hostport_and_query[i + 1..]),
        None => (hostport_and_query, ""),
    };

    let (host, port) = match split_host_port(hostport) {
        Some(pair) => pair,
        None => {
            tracing::warn!("Base64 URI: vless invalid host:port: {hostport}");
            return SubscriptionProxy::Unknown {
                raw_config: format!("vless://{rest}"),
            };
        }
    };

    let params = QueryParams::parse(query);
    let network = params
        .get("type")
        .or_else(|| params.get("network"))
        .unwrap_or_else(|| "tcp".to_string());
    let security = params.get_non_empty("security").filter(|v| v != "none");
    let encryption = params
        .get("encryption")
        .unwrap_or_else(|| "none".to_string());

    SubscriptionProxy::Vless {
        host,
        port,
        uuid: percent_decode(uuid),
        encryption,
        flow: params.get_non_empty("flow"),
        network,
        security,
        sni: params
            .get_non_empty("sni")
            .or_else(|| params.get_non_empty("servername")),
        host_header: params.get_non_empty("host"),
        path: params.get_non_empty("path"),
        service_name: params
            .get_non_empty("serviceName")
            .or_else(|| params.get_non_empty("service_name")),
        fingerprint: params
            .get_non_empty("fp")
            .or_else(|| params.get_non_empty("fingerprint")),
        public_key: params
            .get_non_empty("pbk")
            .or_else(|| params.get_non_empty("publicKey"))
            .or_else(|| params.get_non_empty("public-key")),
        short_id: params
            .get_non_empty("sid")
            .or_else(|| params.get_non_empty("shortId"))
            .or_else(|| params.get_non_empty("short-id")),
        spider_x: params
            .get_non_empty("spx")
            .or_else(|| params.get_non_empty("spiderX"))
            .or_else(|| params.get_non_empty("spider-x")),
    }
}

struct QueryParams {
    map: std::collections::HashMap<String, String>,
}

impl QueryParams {
    fn parse(query: &str) -> Self {
        let map = url::form_urlencoded::parse(query.as_bytes())
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        Self { map }
    }

    fn get(&self, key: &str) -> Option<String> {
        self.map.get(key).cloned()
    }

    fn get_non_empty(&self, key: &str) -> Option<String> {
        self.get(key).filter(|value| !value.trim().is_empty())
    }
}

/// Split `host:port` into `(host, port)`.
fn split_host_port(s: &str) -> Option<(String, u16)> {
    // Handle IPv6: [::1]:port
    if let Some(close) = s.find(']') {
        // IPv6 format: [host]:port
        let host = &s[..=close];
        let port_part = s.get(close + 1..)?.strip_prefix(':')?;
        Some((host.to_string(), port_part.parse().ok()?))
    } else {
        // Regular: host:port — split on last ':'
        let idx = s.rfind(':')?;
        let host = &s[..idx];
        let port: u16 = s[idx + 1..].parse().ok()?;
        Some((host.to_string(), port))
    }
}

/// Minimal percent-decode for trojan passwords.
///
/// Decodes `%XX` escapes into bytes and interprets the result as UTF-8
/// (lossy), so multi-byte credentials such as `%E4%B8%AD` round-trip
/// correctly instead of being split into per-byte Latin-1 chars.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            // Parse the two hex bytes directly instead of slicing `s`: a bare
            // '%' followed by a multi-byte UTF-8 char (e.g. `%中`) would make
            // `&s[i + 1..i + 3]` land mid-character and panic on the non-char
            // boundary. ASCII hex digits round-trip through `u8 as char`.
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    // -- decode_base64 --
    #[test]
    fn test_decode_base64_standard() {
        // "ss://abc" base64 standard
        let encoded = STANDARD.encode("ss://abc");
        assert_eq!(decode_base64(&encoded).unwrap(), "ss://abc");
    }

    #[test]
    fn test_decode_base64_url_safe() {
        let encoded = URL_SAFE.encode("vmess://test");
        assert_eq!(decode_base64(&encoded).unwrap(), "vmess://test");
    }

    #[test]
    fn test_decode_base64_missing_padding() {
        // Remove trailing padding
        let encoded = STANDARD.encode("ss://hello");
        let no_pad = encoded.trim_end_matches('=');
        assert_eq!(decode_base64(no_pad).unwrap(), "ss://hello");
    }

    #[test]
    fn test_pad_base64() {
        assert_eq!(pad_base64("YQ"), "YQ=="); // len=2 → +2
        assert_eq!(pad_base64("YWI"), "YWI="); // len=3 → +1
        assert_eq!(pad_base64("YWIz"), "YWIz"); // len=4 → +0
    }

    // -- split_host_port --
    #[test]
    fn test_split_host_port_ipv4() {
        assert_eq!(
            split_host_port("1.2.3.4:443"),
            Some(("1.2.3.4".into(), 443))
        );
    }

    #[test]
    fn test_split_host_port_ipv6() {
        assert_eq!(split_host_port("[::1]:443"), Some(("[::1]".into(), 443)));
    }

    #[test]
    fn test_split_host_port_invalid() {
        assert!(split_host_port("noport").is_none());
    }

    // -- parse basic --
    #[test]
    fn test_parse_socks5() {
        let result = parse_uri("socks5://10.0.0.1:1080");
        if let SubscriptionProxy::Basic {
            host,
            port,
            protocol,
        } = &result
        {
            assert_eq!(host, "10.0.0.1");
            assert_eq!(*port, 1080);
            assert_eq!(*protocol, Protocol::Socks5);
        } else {
            panic!("Expected Basic, got {result:?}");
        }
    }

    #[test]
    fn test_parse_http() {
        let result = parse_uri("http://10.0.0.2:8080");
        if let SubscriptionProxy::Basic {
            host,
            port,
            protocol,
        } = &result
        {
            assert_eq!(host, "10.0.0.2");
            assert_eq!(*port, 8080);
            assert_eq!(*protocol, Protocol::Http);
        } else {
            panic!("Expected Basic, got {result:?}");
        }
    }

    // -- parse ss --
    #[test]
    fn test_parse_ss_base64_userinfo() {
        // method:password base64-encoded
        let userinfo = STANDARD.encode("aes-256-gcm:mypassword");
        let uri = format!("ss://{userinfo}@10.0.0.3:8388");
        let result = parse_uri(&uri);
        if let SubscriptionProxy::Shadowsocks {
            host,
            port,
            method,
            password,
            plugin,
            plugin_opts,
            ..
        } = &result
        {
            assert_eq!(host, "10.0.0.3");
            assert_eq!(*port, 8388);
            assert_eq!(method, "aes-256-gcm");
            assert_eq!(password, "mypassword");
            assert!(plugin.is_none());
            assert!(plugin_opts.is_none());
        } else {
            panic!("Expected Shadowsocks, got {result:?}");
        }
    }

    #[test]
    fn test_parse_ss_plain_userinfo() {
        let uri = "ss://aes-128-gcm:plainpass@10.0.0.4:8388";
        let result = parse_uri(uri);
        if let SubscriptionProxy::Shadowsocks {
            method, password, ..
        } = &result
        {
            assert_eq!(method, "aes-128-gcm");
            assert_eq!(password, "plainpass");
        } else {
            panic!("Expected Shadowsocks, got {result:?}");
        }
    }

    #[test]
    fn test_parse_ss_with_plugin() {
        let userinfo = STANDARD.encode("aes-256-gcm:mypassword");
        let uri = format!("ss://{userinfo}@10.0.0.5:8388?plugin=obfs-local&plugin-opts=obfs=http");
        let result = parse_uri(&uri);
        if let SubscriptionProxy::Shadowsocks {
            plugin,
            plugin_opts,
            ..
        } = &result
        {
            assert_eq!(plugin.as_deref(), Some("obfs-local"));
            assert_eq!(plugin_opts.as_deref(), Some("obfs=http"));
        } else {
            panic!("Expected Shadowsocks, got {result:?}");
        }
    }

    #[test]
    fn test_parse_ss_with_fragment() {
        let userinfo = STANDARD.encode("aes-256-gcm:mypassword");
        let uri = format!("ss://{userinfo}@10.0.0.5:8388#my-node");
        let result = parse_uri(&uri);
        if let SubscriptionProxy::Shadowsocks { port, .. } = &result {
            assert_eq!(*port, 8388);
        } else {
            panic!("Expected Shadowsocks, got {result:?}");
        }
    }

    // -- parse vmess --
    #[test]
    fn test_parse_vmess() {
        let json = serde_json::json!({
            "v": "2",
            "ps": "test-node",
            "add": "10.0.0.4",
            "port": "443",
            "id": "a3482e88-686a-4a58-8126-99c9df64b7bf",
            "aid": "0",
            "scy": "auto",
            "net": "ws",
            "path": "/v2ray",
            "host": "vmess.example.com",
            "sni": "sni.example.com"
        });
        let encoded = STANDARD.encode(json.to_string());
        let uri = format!("vmess://{encoded}");
        let result = parse_uri(&uri);
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
            ..
        } = &result
        {
            assert_eq!(host, "10.0.0.4");
            assert_eq!(*port, 443);
            assert_eq!(uuid, "a3482e88-686a-4a58-8126-99c9df64b7bf");
            assert_eq!(*alter_id, 0);
            assert_eq!(security, "auto");
            assert_eq!(network, "ws");
            assert_eq!(path.as_deref(), Some("/v2ray"));
            assert_eq!(host_header.as_deref(), Some("vmess.example.com"));
            assert_eq!(sni.as_deref(), Some("sni.example.com"));
        } else {
            panic!("Expected Vmess, got {result:?}");
        }
    }

    #[test]
    fn test_parse_vmess_numeric_port() {
        // Many real-world vmess links encode port as a JSON number, not a string.
        let json = serde_json::json!({
            "v": "2",
            "ps": "numeric-port",
            "add": "10.0.0.5",
            "port": 443,
            "id": "a3482e88-686a-4a58-8126-99c9df64b7bf",
            "aid": 0,
            "net": "tcp"
        });
        let encoded = STANDARD.encode(json.to_string());
        let uri = format!("vmess://{encoded}");
        let result = parse_uri(&uri);
        if let SubscriptionProxy::Vmess { host, port, .. } = &result {
            assert_eq!(host, "10.0.0.5");
            assert_eq!(*port, 443);
        } else {
            panic!("Expected Vmess, got {result:?}");
        }
    }

    #[test]
    fn test_parse_vmess_alt_keys() {
        let json = serde_json::json!({
            "v": "2",
            "ps": "alt-keys",
            "hnb": "10.0.0.10",
            "pnt": "8443",
            "uid": "alt-uuid",
            "aid": "1"
        });
        let encoded = STANDARD.encode(json.to_string());
        let uri = format!("vmess://{encoded}");
        let result = parse_uri(&uri);
        if let SubscriptionProxy::Vmess {
            host,
            port,
            uuid,
            alter_id,
            security,
            network,
            ..
        } = &result
        {
            assert_eq!(host, "10.0.0.10");
            assert_eq!(*port, 8443);
            assert_eq!(uuid, "alt-uuid");
            assert_eq!(*alter_id, 1);
            assert_eq!(security, "auto"); // default
            assert_eq!(network, "tcp"); // default
        } else {
            panic!("Expected Vmess, got {result:?}");
        }
    }

    #[test]
    fn test_parse_vmess_defaults() {
        let json = serde_json::json!({
            "v": "2",
            "add": "10.0.0.4",
            "port": "443",
            "id": "some-uuid"
        });
        let encoded = STANDARD.encode(json.to_string());
        let uri = format!("vmess://{encoded}");
        let result = parse_uri(&uri);
        if let SubscriptionProxy::Vmess {
            security,
            network,
            path,
            host_header,
            sni,
            ..
        } = &result
        {
            assert_eq!(security, "auto");
            assert_eq!(network, "tcp");
            assert!(path.is_none());
            assert!(host_header.is_none());
            assert!(sni.is_none());
        } else {
            panic!("Expected Vmess, got {result:?}");
        }
    }

    // -- parse trojan --
    #[test]
    fn test_parse_trojan() {
        let uri = "trojan://mypassword@10.0.0.5:443?sni=trojan.example.com&type=tcp";
        let result = parse_uri(uri);
        if let SubscriptionProxy::Trojan {
            host,
            port,
            password,
            sni,
            network,
            ..
        } = &result
        {
            assert_eq!(host, "10.0.0.5");
            assert_eq!(*port, 443);
            assert_eq!(password, "mypassword");
            assert_eq!(sni.as_deref(), Some("trojan.example.com"));
            assert_eq!(network.as_deref(), Some("tcp"));
        } else {
            panic!("Expected Trojan, got {result:?}");
        }
    }

    #[test]
    fn test_parse_trojan_no_query() {
        let uri = "trojan://simplepass@10.0.0.6:443";
        let result = parse_uri(uri);
        if let SubscriptionProxy::Trojan {
            host,
            port,
            password,
            sni,
            network,
            ..
        } = &result
        {
            assert_eq!(host, "10.0.0.6");
            assert_eq!(*port, 443);
            assert_eq!(password, "simplepass");
            assert!(sni.is_none());
            assert!(network.is_none());
        } else {
            panic!("Expected Trojan, got {result:?}");
        }
    }

    // -- parse vless --
    #[test]
    fn test_parse_vless_ws_tls() {
        let result = parse_uri(
            "vless://uuid@host.example.com:443?type=ws&security=tls&sni=sni.example.com&host=cdn.example.com&path=%2Fws&flow=xtls-rprx-vision&fp=chrome",
        );
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
        } = &result
        {
            assert_eq!(host, "host.example.com");
            assert_eq!(*port, 443);
            assert_eq!(uuid, "uuid");
            assert_eq!(encryption, "none");
            assert_eq!(flow.as_deref(), Some("xtls-rprx-vision"));
            assert_eq!(network, "ws");
            assert_eq!(security.as_deref(), Some("tls"));
            assert_eq!(sni.as_deref(), Some("sni.example.com"));
            assert_eq!(host_header.as_deref(), Some("cdn.example.com"));
            assert_eq!(path.as_deref(), Some("/ws"));
            assert_eq!(fingerprint.as_deref(), Some("chrome"));
        } else {
            panic!("Expected Vless, got {result:?}");
        }
    }

    #[test]
    fn test_parse_vless_reality() {
        let result = parse_uri(
            "vless://uuid@reality.example.com:443?type=tcp&security=reality&sni=www.microsoft.com&pbk=public-key&sid=abcd&spx=%2F&fp=chrome",
        );
        if let SubscriptionProxy::Vless {
            security,
            sni,
            public_key,
            short_id,
            spider_x,
            fingerprint,
            ..
        } = &result
        {
            assert_eq!(security.as_deref(), Some("reality"));
            assert_eq!(sni.as_deref(), Some("www.microsoft.com"));
            assert_eq!(public_key.as_deref(), Some("public-key"));
            assert_eq!(short_id.as_deref(), Some("abcd"));
            assert_eq!(spider_x.as_deref(), Some("/"));
            assert_eq!(fingerprint.as_deref(), Some("chrome"));
        } else {
            panic!("Expected Vless, got {result:?}");
        }
    }

    #[test]
    fn test_parse_vless_malformed_unknown() {
        let result = parse_uri("vless://uuid-without-host");
        assert!(matches!(result, SubscriptionProxy::Unknown { .. }));
    }

    // -- Unknown scheme --
    #[test]
    fn test_unknown_scheme() {
        let result = parse_uri("random://stuff");
        assert!(matches!(result, SubscriptionProxy::Unknown { .. }));
    }

    // -- Full line decoding --
    #[test]
    fn test_parse_line_base64_encoded() {
        let uri = "ss://aes-256-gcm:pass@1.2.3.4:8388";
        let encoded = STANDARD.encode(uri);
        let result = parse_line(&encoded);
        if let SubscriptionProxy::Shadowsocks { host, .. } = &result {
            assert_eq!(host, "1.2.3.4");
        } else {
            panic!("Expected Shadowsocks, got {result:?}");
        }
    }

    // -- detect --
    #[test]
    fn test_detect_plain_lines() {
        let content = "ss://aes-256-gcm:pass@1.2.3.4:8388\nsocks5://5.6.7.8:1080";
        assert!(Base64UriParser.detect(content));
    }

    #[test]
    fn test_detect_base64_blob() {
        let lines = "ss://aes-256-gcm:pass@1.2.3.4:8388\nsocks5://5.6.7.8:1080";
        let blob = STANDARD.encode(lines);
        assert!(Base64UriParser.detect(&blob));
    }

    #[test]
    fn test_detect_negative() {
        assert!(!Base64UriParser.detect("hello world\nfoo bar"));
        assert!(!Base64UriParser.detect(""));
    }

    // -- full parse with base64 blob --
    #[test]
    fn test_parse_blob() {
        let lines = "ss://aes-256-gcm:pass@1.2.3.4:8388\nsocks5://5.6.7.8:1080";
        let blob = STANDARD.encode(lines);
        let parser = Base64UriParser;
        let proxies = parser.parse(&blob);
        assert_eq!(proxies.len(), 2);
    }

    // -- test fixtures --
    #[test]
    fn test_fixture_base64_sample() {
        let content = include_str!("../../tests/fixtures/base64_sample.txt");
        let parser = Base64UriParser;
        assert!(parser.detect(content));
        let proxies = parser.parse(content);
        // 4 lines, 4 proxies
        assert_eq!(proxies.len(), 4);

        // First: socks5 Basic
        assert!(matches!(
            &proxies[0],
            SubscriptionProxy::Basic {
                protocol: Protocol::Socks5,
                ..
            }
        ));

        // Second: ss Shadowsocks
        assert!(matches!(&proxies[1], SubscriptionProxy::Shadowsocks { .. }));

        // Third: vmess Vmess
        assert!(matches!(&proxies[2], SubscriptionProxy::Vmess { .. }));

        // Fourth: trojan Trojan
        assert!(matches!(&proxies[3], SubscriptionProxy::Trojan { .. }));
    }

    #[test]
    fn test_fixture_base64_blob() {
        let content = include_str!("../../tests/fixtures/base64_blob.txt");
        let parser = Base64UriParser;
        assert!(parser.detect(content));
        let proxies = parser.parse(content);
        // Same 4 URIs as the sample, but encoded as one blob
        assert_eq!(proxies.len(), 4);
        assert!(matches!(
            &proxies[0],
            SubscriptionProxy::Basic {
                protocol: Protocol::Socks5,
                ..
            }
        ));
        assert!(matches!(&proxies[1], SubscriptionProxy::Shadowsocks { .. }));
        assert!(matches!(&proxies[2], SubscriptionProxy::Vmess { .. }));
        assert!(matches!(&proxies[3], SubscriptionProxy::Trojan { .. }));
    }

    // -- percent decode --
    #[test]
    fn test_percent_decode() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("no%2Fencode"), "no/encode");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("%ZZinvalid"), "%ZZinvalid");
        // Multi-byte UTF-8 must round-trip (中 = %E4%B8%AD).
        assert_eq!(percent_decode("%E4%B8%AD"), "中");
        assert_eq!(percent_decode("p%40ss"), "p@ss");
        // A bare '%' before a multi-byte UTF-8 char must not panic on a
        // non-char-boundary slice; it is passed through unchanged.
        assert_eq!(percent_decode("%中"), "%中");
        assert_eq!(percent_decode("a%🌐b"), "a%🌐b");
    }

    // -- SS with plugin query --
    #[test]
    fn test_parse_ss_query() {
        let (plugin, opts) = parse_ss_query("plugin=obfs-local&plugin-opts=obfs%3Dhttp");
        assert_eq!(plugin.as_deref(), Some("obfs-local"));
        assert_eq!(opts.as_deref(), Some("obfs%3Dhttp"));
    }

    #[test]
    fn test_parse_ss_query_empty() {
        let (plugin, opts) = parse_ss_query("");
        assert!(plugin.is_none());
        assert!(opts.is_none());
    }

    // -- malformed inputs --
    #[test]
    fn test_parse_ss_no_at() {
        let result = parse_uri("ss://nouserinfo");
        assert!(matches!(result, SubscriptionProxy::Unknown { .. }));
    }

    #[test]
    fn test_parse_vmess_bad_base64() {
        let result = parse_uri("vmess://!!!not-base64!!!");
        assert!(matches!(result, SubscriptionProxy::Unknown { .. }));
    }

    #[test]
    fn test_parse_trojan_percent_password() {
        let result = parse_uri("trojan://pass%2Fword@1.2.3.4:443");
        if let SubscriptionProxy::Trojan { password, .. } = &result {
            assert_eq!(password, "pass/word");
        } else {
            panic!("Expected Trojan, got {result:?}");
        }
    }
}
