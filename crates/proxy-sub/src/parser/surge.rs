//! Surge subscription parser.
//!
//! Parses the Surge proxy list format:
//! `Name = type, server, port, key1=value1, key2=value2, ...`
//!
//! Supported types: socks5, http, ss, vmess, trojan.
//! Unsupported types map to `Unknown`.

use crate::models::SubscriptionProxy;
use crate::parser::Parser;
use proxy_core::models::Protocol;

/// Known Surge proxy type identifiers.
const KNOWN_TYPES: &[&str] = &["socks5", "http", "ss", "vmess", "trojan", "vless"];

/// Surge format parser.
pub struct SurgeParser;

impl Parser for SurgeParser {
    fn name(&self) -> &str {
        "Surge"
    }

    fn detect(&self, content: &str) -> bool {
        for line in content.lines() {
            let line = line.trim();
            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if is_surge_line(line) {
                return true;
            }
        }
        false
    }

    fn parse(&self, content: &str) -> Vec<SubscriptionProxy> {
        content
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                // Skip empty lines and comments
                if line.is_empty() || line.starts_with('#') {
                    return None;
                }
                Some(parse_line(line))
            })
            .collect()
    }
}

/// Check whether a single line matches the Surge pattern:
/// `Name = type, server, port, [params...]`
fn is_surge_line(line: &str) -> bool {
    // Must contain '=' separating name from the rest
    let Some((_name, rest)) = line.split_once('=') else {
        return false;
    };
    let rest = rest.trim();

    // Split on first comma to extract the type field
    let Some(type_field) = rest.split(',').next() else {
        return false;
    };
    let type_field = type_field.trim().to_ascii_lowercase();

    KNOWN_TYPES.contains(&type_field.as_str())
}

/// Parse a single Surge proxy line into a `SubscriptionProxy`.
fn parse_line(line: &str) -> SubscriptionProxy {
    // Split "Name = type, server, port, params..."
    let Some((_name, rest)) = line.split_once('=') else {
        tracing::warn!("Surge: line has no '=' separator: {line}");
        return SubscriptionProxy::Unknown {
            raw_config: line.to_string(),
        };
    };
    let rest = rest.trim();

    // splitn(4, ','): type, server, port, all-params
    let parts: Vec<&str> = rest.splitn(4, ',').collect();
    if parts.len() < 3 {
        tracing::warn!("Surge: line has fewer than 3 comma-separated fields: {line}");
        return SubscriptionProxy::Unknown {
            raw_config: line.to_string(),
        };
    }

    let proxy_type = parts[0].trim().to_ascii_lowercase();
    let server = parts[1].trim().to_string();
    let port: u16 = match parts[2].trim().parse() {
        Ok(p) => p,
        Err(_) => {
            tracing::warn!("Surge: invalid port '{}' in line: {line}", parts[2].trim());
            return SubscriptionProxy::Unknown {
                raw_config: line.to_string(),
            };
        }
    };

    // Parse key=value params from the 4th field (if present)
    let params = if parts.len() > 3 {
        Params::parse(parts[3].trim())
    } else {
        Params::default()
    };

    match proxy_type.as_str() {
        "socks5" => SubscriptionProxy::Basic {
            host: server,
            port,
            protocol: Protocol::Socks5,
        },
        "http" => SubscriptionProxy::Basic {
            host: server,
            port,
            protocol: Protocol::Http,
        },
        "ss" => SubscriptionProxy::Shadowsocks {
            host: server,
            port,
            method: params.get_or("encrypt-method", "aes-256-gcm"),
            password: params.get_or("password", ""),
            plugin: params.get("plugin"),
            plugin_opts: params.get("plugin-opts"),
        },
        "vmess" => {
            let network = if params.is("ws", "true") {
                "ws".to_string()
            } else {
                params.get("network").unwrap_or_else(|| "tcp".to_string())
            };
            SubscriptionProxy::Vmess {
                host: server,
                port,
                uuid: params.get_or("username", ""),
                alter_id: params
                    .get("alter-id")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
                security: params.get_or("security", "auto"),
                network,
                path: params.get("ws-path"),
                host_header: params.get("ws-host"),
                sni: params.get("sni"),
            }
        }
        "trojan" => SubscriptionProxy::Trojan {
            host: server,
            port,
            password: params.get_or("password", ""),
            sni: params.get("sni"),
            network: params.get("network"),
        },
        _ => SubscriptionProxy::Unknown {
            raw_config: line.to_string(),
        },
    }
}

/// Parsed key=value parameters from a Surge line.
#[derive(Default)]
struct Params {
    map: std::collections::HashMap<String, String>,
}

impl Params {
    /// Parse a comma-separated list of `key=value` pairs.
    fn parse(input: &str) -> Self {
        let mut map = std::collections::HashMap::new();
        for pair in input.split(',') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            if let Some((key, value)) = pair.split_once('=') {
                map.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
        Self { map }
    }

    /// Get a parameter value by key.
    fn get(&self, key: &str) -> Option<String> {
        self.map.get(key).cloned()
    }

    /// Get a parameter value by key, or a default.
    fn get_or(&self, key: &str, default: &str) -> String {
        self.map
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }

    /// Check if a parameter equals a specific value.
    fn is(&self, key: &str, value: &str) -> bool {
        self.map
            .get(key)
            .is_some_and(|v| v.eq_ignore_ascii_case(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- detect --
    #[test]
    fn test_detect_valid_surge() {
        let parser = SurgeParser;
        let content = "socks5-proxy = socks5, 10.0.0.1, 1080\nhttp-proxy = http, 10.0.0.2, 8080";
        assert!(parser.detect(content));
    }

    #[test]
    fn test_detect_single_line() {
        let parser = SurgeParser;
        assert!(
            parser
                .detect("my-proxy = ss, 1.2.3.4, 8388, encrypt-method=aes-256-gcm, password=pass")
        );
    }

    #[test]
    fn test_detect_invalid() {
        let parser = SurgeParser;
        assert!(!parser.detect("just some random text"));
        assert!(!parser.detect("hello world\nfoo bar"));
        assert!(!parser.detect(""));
    }

    #[test]
    fn test_detect_skips_comments() {
        let parser = SurgeParser;
        let content = "# This is a comment\nsocks5-proxy = socks5, 10.0.0.1, 1080";
        assert!(parser.detect(content));
    }

    #[test]
    fn test_detect_unknown_type() {
        let parser = SurgeParser;
        // A line with '=' but unknown type should not match
        assert!(!parser.detect("my-proxy = unknown-type, 1.2.3.4, 443"));
    }

    // -- parse: socks5 --
    #[test]
    fn test_parse_socks5() {
        let parser = SurgeParser;
        let proxies = parser.parse("my-socks = socks5, 10.0.0.1, 1080");
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Basic {
            host,
            port,
            protocol,
        } = &proxies[0]
        {
            assert_eq!(host, "10.0.0.1");
            assert_eq!(*port, 1080);
            assert_eq!(*protocol, Protocol::Socks5);
        } else {
            panic!("Expected Basic, got {:?}", proxies[0]);
        }
    }

    // -- parse: http --
    #[test]
    fn test_parse_http() {
        let parser = SurgeParser;
        let proxies = parser.parse("my-http = http, 10.0.0.2, 8080");
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Basic {
            host,
            port,
            protocol,
        } = &proxies[0]
        {
            assert_eq!(host, "10.0.0.2");
            assert_eq!(*port, 8080);
            assert_eq!(*protocol, Protocol::Http);
        } else {
            panic!("Expected Basic, got {:?}", proxies[0]);
        }
    }

    // -- parse: ss --
    #[test]
    fn test_parse_ss() {
        let parser = SurgeParser;
        let proxies = parser.parse(
            "ss-proxy = ss, 10.0.0.3, 8388, encrypt-method=aes-256-gcm, password=mypassword",
        );
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Shadowsocks {
            host,
            port,
            method,
            password,
            plugin,
            plugin_opts,
            ..
        } = &proxies[0]
        {
            assert_eq!(host, "10.0.0.3");
            assert_eq!(*port, 8388);
            assert_eq!(method, "aes-256-gcm");
            assert_eq!(password, "mypassword");
            assert!(plugin.is_none());
            assert!(plugin_opts.is_none());
        } else {
            panic!("Expected Shadowsocks, got {:?}", proxies[0]);
        }
    }

    // -- parse: vmess with ws --
    #[test]
    fn test_parse_vmess_ws() {
        let parser = SurgeParser;
        let proxies = parser.parse(
            "vmess-proxy = vmess, 10.0.0.4, 443, username=a3482e88-686a-4a58-8126-99c9df64b7bf, tls=true, ws=true, ws-path=/v2ray, ws-host=vmess.example.com, sni=vmess.example.com",
        );
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
            ..
        } = &proxies[0]
        {
            assert_eq!(host, "10.0.0.4");
            assert_eq!(*port, 443);
            assert_eq!(uuid, "a3482e88-686a-4a58-8126-99c9df64b7bf");
            assert_eq!(*alter_id, 0);
            assert_eq!(security, "auto");
            assert_eq!(network, "ws");
            assert_eq!(path.as_deref(), Some("/v2ray"));
            assert_eq!(host_header.as_deref(), Some("vmess.example.com"));
            assert_eq!(sni.as_deref(), Some("vmess.example.com"));
        } else {
            panic!("Expected Vmess, got {:?}", proxies[0]);
        }
    }

    // -- parse: vmess without ws (default tcp) --
    #[test]
    fn test_parse_vmess_tcp() {
        let parser = SurgeParser;
        let proxies = parser.parse("vmess-tcp = vmess, 10.0.0.10, 443, username=some-uuid");
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

    // -- parse: trojan --
    #[test]
    fn test_parse_trojan() {
        let parser = SurgeParser;
        let proxies = parser.parse(
            "trojan-proxy = trojan, 10.0.0.5, 443, password=trojanpass, sni=trojan.example.com",
        );
        assert_eq!(proxies.len(), 1);
        if let SubscriptionProxy::Trojan {
            host,
            port,
            password,
            sni,
            network,
            ..
        } = &proxies[0]
        {
            assert_eq!(host, "10.0.0.5");
            assert_eq!(*port, 443);
            assert_eq!(password, "trojanpass");
            assert_eq!(sni.as_deref(), Some("trojan.example.com"));
            assert!(network.is_none());
        } else {
            panic!("Expected Trojan, got {:?}", proxies[0]);
        }
    }

    // -- parse: unknown type --
    #[test]
    fn test_parse_unknown_type() {
        let parser = SurgeParser;
        let proxies = parser.parse("weird = hysteria2, 10.0.0.9, 443");
        assert_eq!(proxies.len(), 1);
        assert!(matches!(&proxies[0], SubscriptionProxy::Unknown { .. }));
    }

    // -- parse: malformed lines --
    #[test]
    fn test_parse_malformed_no_equals() {
        let parser = SurgeParser;
        let proxies = parser.parse("this is not a surge line");
        assert_eq!(proxies.len(), 1);
        assert!(matches!(&proxies[0], SubscriptionProxy::Unknown { .. }));
    }

    #[test]
    fn test_parse_malformed_too_few_fields() {
        let parser = SurgeParser;
        let proxies = parser.parse("bad = socks5, 10.0.0.1");
        assert_eq!(proxies.len(), 1);
        assert!(matches!(&proxies[0], SubscriptionProxy::Unknown { .. }));
    }

    #[test]
    fn test_parse_malformed_port() {
        let parser = SurgeParser;
        let proxies = parser.parse("bad = socks5, 10.0.0.1, notaport");
        assert_eq!(proxies.len(), 1);
        assert!(matches!(&proxies[0], SubscriptionProxy::Unknown { .. }));
    }

    // -- parse: comments and empty lines --
    #[test]
    fn test_parse_skips_comments_and_empty() {
        let parser = SurgeParser;
        let content = "# comment line\n\nsocks5-proxy = socks5, 10.0.0.1, 1080\n# another comment\nhttp-proxy = http, 10.0.0.2, 8080\n";
        let proxies = parser.parse(content);
        assert_eq!(proxies.len(), 2);
    }

    // -- test fixture --
    #[test]
    fn test_fixture_surge_sample() {
        let content = include_str!("../../tests/fixtures/surge_sample.txt");
        let parser = SurgeParser;
        assert!(parser.detect(content));
        let proxies = parser.parse(content);
        assert_eq!(proxies.len(), 5);

        // socks5 -> Basic
        if let SubscriptionProxy::Basic {
            host,
            port,
            protocol,
        } = &proxies[0]
        {
            assert_eq!(host, "10.0.0.1");
            assert_eq!(*port, 1080);
            assert_eq!(*protocol, Protocol::Socks5);
        } else {
            panic!("Expected Basic, got {:?}", proxies[0]);
        }

        // http -> Basic
        if let SubscriptionProxy::Basic {
            host,
            port,
            protocol,
        } = &proxies[1]
        {
            assert_eq!(host, "10.0.0.2");
            assert_eq!(*port, 8080);
            assert_eq!(*protocol, Protocol::Http);
        } else {
            panic!("Expected Basic, got {:?}", proxies[1]);
        }

        // ss -> Shadowsocks
        if let SubscriptionProxy::Shadowsocks {
            host,
            method,
            password,
            ..
        } = &proxies[2]
        {
            assert_eq!(host, "10.0.0.3");
            assert_eq!(method, "aes-256-gcm");
            assert_eq!(password, "mypassword");
        } else {
            panic!("Expected Shadowsocks, got {:?}", proxies[2]);
        }

        // vmess -> Vmess
        if let SubscriptionProxy::Vmess {
            uuid,
            network,
            path,
            host_header,
            sni,
            ..
        } = &proxies[3]
        {
            assert_eq!(uuid, "a3482e88-686a-4a58-8126-99c9df64b7bf");
            assert_eq!(network, "ws");
            assert_eq!(path.as_deref(), Some("/v2ray"));
            assert_eq!(host_header.as_deref(), Some("vmess.example.com"));
            assert_eq!(sni.as_deref(), Some("vmess.example.com"));
        } else {
            panic!("Expected Vmess, got {:?}", proxies[3]);
        }

        // trojan -> Trojan
        if let SubscriptionProxy::Trojan {
            host,
            password,
            sni,
            ..
        } = &proxies[4]
        {
            assert_eq!(host, "10.0.0.5");
            assert_eq!(password, "trojanpass");
            assert_eq!(sni.as_deref(), Some("trojan.example.com"));
        } else {
            panic!("Expected Trojan, got {:?}", proxies[4]);
        }
    }

    // -- Params unit tests --
    #[test]
    fn test_params_parse() {
        let params = Params::parse("encrypt-method=aes-256-gcm, password=mypassword, tls=true");
        assert_eq!(params.get("encrypt-method").as_deref(), Some("aes-256-gcm"));
        assert_eq!(params.get("password").as_deref(), Some("mypassword"));
        assert_eq!(params.get("tls").as_deref(), Some("true"));
        assert!(params.get("nonexistent").is_none());
    }

    #[test]
    fn test_params_is() {
        let params = Params::parse("ws=true, tls=TRUE");
        assert!(params.is("ws", "true"));
        assert!(params.is("tls", "true")); // case-insensitive
        assert!(!params.is("ws", "false"));
    }

    #[test]
    fn test_params_get_or() {
        let params = Params::parse("key=value");
        assert_eq!(params.get_or("key", "default"), "value");
        assert_eq!(params.get_or("missing", "default"), "default");
    }
}
