//! Xray integration data models.

use serde::{Deserialize, Serialize};

/// An active xray node representing an encrypted proxy with a local SOCKS5 port.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XrayNode {
    /// Unique tag: "{protocol_label}-{host}-{port}"
    pub tag: String,
    /// Local SOCKS5 port that xray listens on for this node.
    pub local_socks5_port: u16,
    /// Protocol label: "ss", "vmess", "trojan", "vless"
    pub protocol_label: String,
    /// Original remote host.
    pub remote_host: String,
    /// Original remote port.
    pub remote_port: u16,
    /// Serialized SubscriptionProxy JSON (for re-sync on xray restart).
    pub raw_config: String,
}

impl XrayNode {
    /// Create a new `XrayNode`.
    pub fn new(
        tag: String,
        local_socks5_port: u16,
        protocol_label: &str,
        remote_host: &str,
        remote_port: u16,
        raw_config: String,
    ) -> Self {
        Self {
            tag,
            local_socks5_port,
            protocol_label: protocol_label.to_string(),
            remote_host: remote_host.to_string(),
            remote_port,
            raw_config,
        }
    }

    /// Inbound tag: "in-{tag}"
    pub fn inbound_tag(&self) -> String {
        format!("in-{}", self.tag)
    }

    /// Outbound tag: "out-{tag}"
    pub fn outbound_tag(&self) -> String {
        format!("out-{}", self.tag)
    }

    /// Routing rule tag matching inbound to outbound.
    pub fn routing_rule_tag(&self) -> String {
        crate::config_gen::routing_rule_tag(&self.inbound_tag())
    }
}

/// Result of a single outbound sync cycle.
#[derive(Debug, Default)]
pub struct SyncStats {
    pub added: usize,
    pub removed: usize,
    pub failed: usize,
    pub total_active: usize,
}
