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
    /// VLESS node.
    Vless {
        host: String,
        port: u16,
        uuid: String,
        encryption: String,
        flow: Option<String>,
        network: String,
        security: Option<String>,
        sni: Option<String>,
        host_header: Option<String>,
        path: Option<String>,
        service_name: Option<String>,
        fingerprint: Option<String>,
        public_key: Option<String>,
        short_id: Option<String>,
        spider_x: Option<String>,
    },
    /// Unknown or unsupported protocol.
    Unknown { raw_config: String },
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
            Self::Vless { host, .. } => Some(host),
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
            Self::Vless { port, .. } => Some(*port),
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
            Self::Vless { .. } => "vless",
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

        let vless = SubscriptionProxy::Vless {
            host: "8.8.8.8".into(),
            port: 443,
            uuid: "uid".into(),
            encryption: "none".into(),
            flow: Some("xtls-rprx-vision".into()),
            network: "tcp".into(),
            security: Some("reality".into()),
            sni: Some("example.com".into()),
            host_header: None,
            path: None,
            service_name: None,
            fingerprint: Some("chrome".into()),
            public_key: Some("public-key".into()),
            short_id: Some("abcd".into()),
            spider_x: Some("/".into()),
        };
        assert_eq!(vless.protocol_label(), "vless");
        assert_eq!(vless.dedup_key(), "8.8.8.8:443:vless");
    }
}
