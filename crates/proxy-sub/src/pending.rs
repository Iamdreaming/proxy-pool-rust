//! Pending storage: encrypted proxy nodes awaiting xray relay setup.
//!
//! Stores `SubscriptionProxy` nodes in Redis ZSets keyed by protocol label,
//! scored by Unix timestamp so that newer entries sort higher.

use crate::models::SubscriptionProxy;
use anyhow::Result;
use chrono::Utc;
use redis::AsyncCommands;
use redis::aio::MultiplexedConnection;

/// Redis-backed store for encrypted proxy nodes that cannot directly join the
/// pool and need xray relay configuration first.
pub struct PendingStore {
    conn: MultiplexedConnection,
}

impl PendingStore {
    /// Create a new `PendingStore` backed by the given Redis connection.
    pub fn new(conn: MultiplexedConnection) -> Self {
        Self { conn }
    }

    fn redis_key(protocol_label: &str) -> String {
        format!("pending:encrypted:{protocol_label}")
    }

    /// Store a batch of encrypted proxy nodes into the appropriate ZSets.
    ///
    /// Each node is serialized as JSON and stored in a ZSet keyed by its
    /// protocol label. The ZSet score is the current Unix timestamp.
    pub async fn store_batch(&self, nodes: &[SubscriptionProxy]) -> Result<()> {
        let now = Utc::now().timestamp();
        for node in nodes {
            let key = Self::redis_key(node.protocol_label());
            let member = serde_json::to_string(node)?;
            let mut conn = self.conn.clone();
            let _: () = conn.zadd(&key, &member, now).await?;
        }
        Ok(())
    }

    /// Retrieve pending nodes for a given protocol label, most recent first.
    pub async fn get_pending(
        &self,
        protocol_label: &str,
        limit: usize,
    ) -> Result<Vec<SubscriptionProxy>> {
        let key = Self::redis_key(protocol_label);
        let mut conn = self.conn.clone();
        let members: Vec<String> = conn.zrevrange(&key, 0, limit as isize - 1).await?;
        let mut result = Vec::with_capacity(members.len());
        for m in members {
            match serde_json::from_str::<SubscriptionProxy>(&m) {
                Ok(p) => result.push(p),
                Err(e) => tracing::warn!("failed to parse pending proxy from redis: {e}"),
            }
        }
        Ok(result)
    }

    /// Count the number of pending nodes for a given protocol label.
    pub async fn count_pending(&self, protocol_label: &str) -> Result<usize> {
        let key = Self::redis_key(protocol_label);
        let mut conn = self.conn.clone();
        let c: u64 = conn.zcard(&key).await?;
        Ok(c as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxy_core::models::Protocol;

    #[test]
    fn test_subscription_proxy_serialization_roundtrip() {
        let sub = SubscriptionProxy::Shadowsocks {
            host: "5.6.7.8".into(),
            port: 8388,
            method: "aes-256-gcm".into(),
            password: "mypassword".into(),
            plugin: Some("obfs-local".into()),
            plugin_opts: None,
        };

        let json = serde_json::to_string(&sub).unwrap();
        let decoded: SubscriptionProxy = serde_json::from_str(&json).unwrap();

        // Verify roundtrip via protocol label (easier than matching all fields)
        assert_eq!(decoded.protocol_label(), "ss");
        assert_eq!(decoded.host(), Some("5.6.7.8"));
        assert_eq!(decoded.port(), Some(8388));

        // Also test Basic roundtrip
        let basic = SubscriptionProxy::Basic {
            host: "1.2.3.4".into(),
            port: 1080,
            protocol: Protocol::Socks5,
        };
        let json = serde_json::to_string(&basic).unwrap();
        let decoded: SubscriptionProxy = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.protocol_label(), "basic");
        assert_eq!(decoded.host(), Some("1.2.3.4"));
        assert_eq!(decoded.port(), Some(1080));

        let vless = SubscriptionProxy::Vless {
            host: "vless.example.com".into(),
            port: 443,
            uuid: "550e8400-e29b-41d4-a716-446655440000".into(),
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
        let json = serde_json::to_string(&vless).unwrap();
        let decoded: SubscriptionProxy = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.protocol_label(), "vless");
        assert_eq!(decoded.host(), Some("vless.example.com"));
        assert_eq!(decoded.port(), Some(443));
    }
}
