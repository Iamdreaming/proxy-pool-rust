//! Background loop that reads pending encrypted nodes from Redis,
//! configures xray-core outbounds, and creates active Proxy entries.

use crate::config_gen::ConfigGenerator;
use crate::models::{SyncStats, XrayNode};
use crate::port_manager::PortManager;
use crate::xray_client::XrayClient;
use proxy_core::config::XraySettings;
use proxy_core::models::{EncryptedProxyState, Protocol, Proxy};
use proxy_core::store::ProxyStore;
use proxy_sub::models::SubscriptionProxy;
use proxy_sub::pending::PendingStore;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Background sync: pending encrypted nodes -> active xray outbounds.
///
/// Reads pending `SubscriptionProxy` nodes from Redis, allocates local SOCKS5
/// ports, generates xray inbound/outbound configs, and registers `Proxy`
/// entries in the pool with `EncryptedProxyState::Active`.
pub struct OutboundSync {
    pending_store: PendingStore,
    proxy_store: Arc<ProxyStore>,
    xray_client: Arc<RwLock<XrayClient>>,
    port_manager: Arc<PortManager>,
    active_nodes: Arc<RwLock<HashMap<String, XrayNode>>>,
    config: XraySettings,
}

impl OutboundSync {
    /// Create a new `OutboundSync`.
    pub fn new(
        pending_store: PendingStore,
        proxy_store: Arc<ProxyStore>,
        xray_client: Arc<RwLock<XrayClient>>,
        port_manager: Arc<PortManager>,
        config: XraySettings,
    ) -> Self {
        Self {
            pending_store,
            proxy_store,
            xray_client,
            port_manager,
            active_nodes: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Run a single sync cycle.
    ///
    /// Iterates over encrypted protocol labels (ss, vmess, trojan), reads
    /// pending nodes from Redis, and activates any that are not yet in the
    /// active set.
    pub async fn sync_once(&self) -> SyncStats {
        let mut stats = SyncStats::default();
        let labels = ["ss", "vmess", "trojan"];

        for label in labels {
            let pending = match self
                .pending_store
                .get_pending(label, self.config.max_active_nodes)
                .await
            {
                Ok(nodes) => nodes,
                Err(e) => {
                    tracing::warn!("outbound_sync: failed to read pending:{label}: {e}");
                    continue;
                }
            };

            for node in &pending {
                let tag = format!(
                    "{}-{}-{}",
                    node.protocol_label(),
                    node.host().unwrap_or("unknown"),
                    node.port().unwrap_or(0)
                );

                // Skip already-active nodes.
                {
                    let active = self.active_nodes.read().await;
                    if active.contains_key(&tag) {
                        continue;
                    }
                }

                // Check capacity.
                let active_count = {
                    let active = self.active_nodes.read().await;
                    active.len()
                };
                if active_count >= self.config.max_active_nodes {
                    tracing::warn!(
                        "outbound_sync: max_active_nodes ({}) reached, skipping",
                        self.config.max_active_nodes
                    );
                    break;
                }

                // Skip nodes that do not produce an xray outbound config
                // (Basic, Unknown — these should not appear in encrypted
                // pending sets, but guard against it).
                if !matches!(
                    node,
                    SubscriptionProxy::Shadowsocks { .. }
                        | SubscriptionProxy::Vmess { .. }
                        | SubscriptionProxy::Trojan { .. }
                ) {
                    continue;
                }

                // Allocate local port.
                let local_port = match self.port_manager.allocate().await {
                    Some(p) => p,
                    None => {
                        tracing::warn!("outbound_sync: port exhaustion, skipping remaining nodes");
                        break;
                    }
                };

                // Generate xray config.
                let node_config = match ConfigGenerator::generate(node, local_port) {
                    Some(c) => c,
                    None => {
                        self.port_manager.release(local_port).await;
                        stats.failed += 1;
                        continue;
                    }
                };

                // Push config to xray via gRPC.
                {
                    let client = self.xray_client.read().await;
                    if client.is_connected() {
                        if let Err(e) = client.add_inbound(&node_config.inbound_json).await {
                            tracing::warn!("outbound_sync: add_inbound failed: {e}");
                        }
                        if let Err(e) = client.add_outbound(&node_config.outbound_json).await {
                            tracing::warn!("outbound_sync: add_outbound failed: {e}");
                        }
                    }
                }

                tracing::info!(
                    "outbound_sync: activated {} -> local port {}",
                    node_config.tag,
                    local_port
                );

                // Build XrayNode record.
                let xray_node = XrayNode::new(
                    tag.clone(),
                    local_port,
                    node.protocol_label(),
                    node.host().unwrap_or("unknown"),
                    node.port().unwrap_or(0),
                    serde_json::to_string(node).unwrap_or_default(),
                );

                // Create Proxy entry in the pool store.
                let mut proxy = Proxy::new("127.0.0.1", local_port, Protocol::Socks5);
                proxy.encrypted_state = Some(EncryptedProxyState::Active {
                    local_socks5_port: local_port,
                });
                proxy.encrypted_config =
                    Some(serde_json::to_value(node).unwrap_or(serde_json::Value::Null));
                proxy.source = Some(format!(
                    "xray:{}:{}",
                    node.protocol_label(),
                    xray_node.remote_host
                ));

                if let Err(e) = self.proxy_store.add(&proxy).await {
                    tracing::warn!("outbound_sync: failed to store proxy: {e}");
                    self.port_manager.release(local_port).await;
                    stats.failed += 1;
                    continue;
                }

                // Record as active.
                {
                    let mut active = self.active_nodes.write().await;
                    active.insert(tag.clone(), xray_node);
                }
                stats.added += 1;
            }

            // Clean up stale nodes: active nodes whose pending entry no longer
            // exists.
            let tags_to_remove: Vec<String> = {
                let active = self.active_nodes.read().await;
                active
                    .keys()
                    .filter(|tag| {
                        // A node is stale if it is for this label but does not
                        // appear in the pending batch.
                        tag.starts_with(label)
                            && !pending.iter().any(|p| {
                                format!(
                                    "{}-{}-{}",
                                    p.protocol_label(),
                                    p.host().unwrap_or("unknown"),
                                    p.port().unwrap_or(0)
                                ) == **tag
                            })
                    })
                    .cloned()
                    .collect()
            };

            for tag in tags_to_remove {
                let node = {
                    let mut active = self.active_nodes.write().await;
                    active.remove(&tag)
                };
                if let Some(node) = node {
                    self.port_manager.release(node.local_socks5_port).await;

                    // Remove from xray via gRPC.
                    let mut client = self.xray_client.write().await;
                    if client.is_connected() {
                        if let Err(e) = client.remove_inbound(&node.inbound_tag()).await {
                            tracing::warn!("outbound_sync: remove_inbound failed: {e}");
                        }
                        if let Err(e) = client.remove_outbound(&node.outbound_tag()).await {
                            tracing::warn!("outbound_sync: remove_outbound failed: {e}");
                        }
                    }

                    tracing::info!("outbound_sync: removed stale node {tag}");
                    stats.removed += 1;
                }
            }
        }

        let active = self.active_nodes.read().await;
        stats.total_active = active.len();

        stats
    }

    /// Run the sync loop continuously.
    pub async fn run(self: Arc<Self>) {
        let interval = std::time::Duration::from_secs(self.config.sync_interval_sec);
        tracing::info!(
            "outbound_sync: starting (interval={}s)",
            self.config.sync_interval_sec
        );

        loop {
            let stats = self.sync_once().await;
            tracing::info!(
                "outbound_sync: cycle complete -- added: {}, removed: {}, failed: {}, total_active: {}",
                stats.added,
                stats.removed,
                stats.failed,
                stats.total_active
            );
            tokio::time::sleep(interval).await;
        }
    }

    /// Get a reference to active nodes (for re-sync on xray restart).
    pub async fn active_nodes(
        &self,
    ) -> tokio::sync::RwLockReadGuard<'_, HashMap<String, XrayNode>> {
        self.active_nodes.read().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_stats_default() {
        let stats = SyncStats::default();
        assert_eq!(stats.added, 0);
        assert_eq!(stats.removed, 0);
        assert_eq!(stats.failed, 0);
        assert_eq!(stats.total_active, 0);
    }
}
