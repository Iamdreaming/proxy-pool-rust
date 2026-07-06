//! Background loop that reads pending encrypted nodes from Redis,
//! configures xray-core outbounds, and creates active Proxy entries.
//!
//! The sync loop pauses when the xray gRPC connection is lost and
//! resumes automatically on reconnection.

use crate::config_gen::{ConfigGenerator, XrayNodeConfig};
use crate::models::{SyncStats, XrayNode};
use crate::port_manager::PortManager;
use crate::xray_client::XrayClient;
use proxy_core::config::XraySettings;
use proxy_core::models::{EncryptedProxyState, Protocol, Proxy};
use proxy_core::store::ProxyStore;
use proxy_core::xray_status::{XrayNodeIdentity, XrayStatusRegistry};
use proxy_sub::models::SubscriptionProxy;
use proxy_sub::pending::PendingStore;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::RwLock;
use tokio::sync::watch;

/// Background sync: pending encrypted nodes -> active xray outbounds.
///
/// Reads pending `SubscriptionProxy` nodes from Redis, allocates local SOCKS5
/// ports, generates xray inbound/outbound configs, and registers `Proxy`
/// entries in the pool with `EncryptedProxyState::Active`.
///
/// The sync is paused when the xray gRPC connection is lost (tracked via
/// `connected_rx` watch channel) and resumes on reconnect.
pub struct OutboundSync {
    pending_store: PendingStore,
    proxy_store: Arc<ProxyStore>,
    xray_client: Arc<RwLock<XrayClient>>,
    port_manager: Arc<PortManager>,
    active_nodes: Arc<RwLock<HashMap<String, XrayNode>>>,
    status_registry: XrayStatusRegistry,
    config: XraySettings,
    /// Watch receiver for xray gRPC connection state.
    connected_rx: watch::Receiver<bool>,
}

impl OutboundSync {
    /// Create a new `OutboundSync`.
    ///
    /// `connected_rx` is a watch channel receiver that tracks the xray gRPC
    /// connection state. The sync loop will pause when disconnected.
    pub fn new(
        pending_store: PendingStore,
        proxy_store: Arc<ProxyStore>,
        xray_client: Arc<RwLock<XrayClient>>,
        port_manager: Arc<PortManager>,
        config: XraySettings,
        connected_rx: watch::Receiver<bool>,
        status_registry: XrayStatusRegistry,
    ) -> Self {
        Self {
            pending_store,
            proxy_store,
            xray_client,
            port_manager,
            active_nodes: Arc::new(RwLock::new(HashMap::new())),
            status_registry,
            config,
            connected_rx,
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
                let identity = xray_identity(tag.clone(), node);

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

                self.status_registry.mark_pending(&identity).await;

                // Allocate local port.
                let local_port = match self.port_manager.allocate().await {
                    Some(p) => p,
                    None => {
                        tracing::warn!("outbound_sync: port exhaustion, skipping remaining nodes");
                        self.status_registry
                            .mark_failed(&identity, None, "port allocation exhausted")
                            .await;
                        stats.failed += 1;
                        break;
                    }
                };
                self.status_registry
                    .mark_activating(&identity, Some(local_port))
                    .await;

                // Generate xray config.
                let node_config = match ConfigGenerator::generate(node, local_port) {
                    Some(c) => c,
                    None => {
                        self.port_manager.release(local_port).await;
                        self.status_registry
                            .mark_failed(
                                &identity,
                                Some(local_port),
                                "xray config generation failed",
                            )
                            .await;
                        stats.failed += 1;
                        continue;
                    }
                };

                // Push required config to xray. Do not mark the node active on
                // partial xray configuration failure.
                if let Err(reason) = self.add_xray_config(&node_config).await {
                    tracing::warn!(
                        "outbound_sync: activate {} failed: {reason}",
                        node_config.tag
                    );
                    self.port_manager.release(local_port).await;
                    self.status_registry
                        .mark_failed(&identity, Some(local_port), reason)
                        .await;
                    stats.failed += 1;
                    continue;
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
                    self.cleanup_xray_config(&node_config).await;
                    self.port_manager.release(local_port).await;
                    self.status_registry
                        .mark_failed(
                            &identity,
                            Some(local_port),
                            format!("failed to store proxy: {e}"),
                        )
                        .await;
                    stats.failed += 1;
                    continue;
                }

                // Record as active.
                {
                    let mut active = self.active_nodes.write().await;
                    active.insert(tag.clone(), xray_node);
                }
                self.status_registry
                    .mark_active(&identity, local_port)
                    .await;
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
                    // Use write lock because remove_inbound takes &mut self.
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
                    self.status_registry
                        .mark_removed(
                            &xray_identity_from_active(&node),
                            Some(node.local_socks5_port),
                        )
                        .await;
                    stats.removed += 1;
                }
            }
        }

        let active = self.active_nodes.read().await;
        stats.total_active = active.len();

        stats
    }

    /// Run a single sync cycle, update the active count, and log the result.
    async fn sync_and_report(&self, context: &str, active_count: &AtomicUsize) {
        let stats = self.sync_once().await;
        active_count.store(stats.total_active, Ordering::Relaxed);
        tracing::info!(
            "outbound_sync: {} -- added: {}, removed: {}, failed: {}, total_active: {}",
            context,
            stats.added,
            stats.removed,
            stats.failed,
            stats.total_active
        );
    }

    /// Run the sync loop continuously, respecting gRPC connection state.
    ///
    /// * If the `connected_rx` watch channel reports `false` (disconnected),
    ///   the sync is skipped with a debug log.
    /// * When the watch channel transitions to `true` (reconnected), an
    ///   immediate sync cycle is triggered.
    /// * Regular sync cycles run at the configured interval.
    pub async fn run(self: Arc<Self>, active_count: Arc<AtomicUsize>) {
        let interval = std::time::Duration::from_secs(self.config.sync_interval_sec);
        tracing::info!(
            "outbound_sync: starting (interval={}s)",
            self.config.sync_interval_sec
        );

        let mut connected_rx = self.connected_rx.clone();

        // Run an initial sync if already connected.
        if *connected_rx.borrow() {
            self.sync_and_report("initial cycle complete", &active_count)
                .await;
        }

        loop {
            tokio::select! {
                _ = tokio::time::sleep(interval) => {
                    if !*connected_rx.borrow() {
                        tracing::debug!("outbound_sync: xray disconnected, skipping sync");
                        continue;
                    }
                    self.sync_and_report("cycle complete", &active_count).await;
                }
                result = connected_rx.changed() => {
                    if result.is_err() {
                        // Sender dropped — connection tracking ended, stop.
                        tracing::info!("outbound_sync: connection tracking ended, stopping");
                        break;
                    }
                    if *connected_rx.borrow() {
                        tracing::info!("outbound_sync: xray reconnected, running immediate sync");
                        self.sync_and_report("reconnection cycle complete", &active_count).await;
                    }
                }
            }
        }

        tracing::info!("outbound_sync: stopped");
    }

    /// Get a reference to active nodes (for re-sync on xray restart).
    pub async fn active_nodes(
        &self,
    ) -> tokio::sync::RwLockReadGuard<'_, HashMap<String, XrayNode>> {
        self.active_nodes.read().await
    }

    async fn add_xray_config(&self, node_config: &XrayNodeConfig) -> Result<(), String> {
        let inbound_result = {
            let client = self.xray_client.read().await;
            if !client.is_connected() {
                return Err("xray gRPC client not connected".into());
            }
            client.add_inbound(&node_config.inbound_json).await
        };
        inbound_result.map_err(|e| format!("add_inbound failed: {e}"))?;

        let outbound_result = {
            let client = self.xray_client.read().await;
            if !client.is_connected() {
                None
            } else {
                Some(client.add_outbound(&node_config.outbound_json).await)
            }
        };

        match outbound_result {
            Some(Ok(())) => Ok(()),
            Some(Err(e)) => {
                self.cleanup_inbound(&node_config.inbound_tag()).await;
                Err(format!("add_outbound failed: {e}"))
            }
            None => {
                self.cleanup_inbound(&node_config.inbound_tag()).await;
                Err("xray gRPC client disconnected after add_inbound".into())
            }
        }
    }

    async fn cleanup_xray_config(&self, node_config: &XrayNodeConfig) {
        let mut client = self.xray_client.write().await;
        if !client.is_connected() {
            return;
        }
        if let Err(e) = client.remove_outbound(&node_config.outbound_tag()).await {
            tracing::warn!("outbound_sync: cleanup remove_outbound failed: {e}");
        }
        if let Err(e) = client.remove_inbound(&node_config.inbound_tag()).await {
            tracing::warn!("outbound_sync: cleanup remove_inbound failed: {e}");
        }
    }

    async fn cleanup_inbound(&self, inbound_tag: &str) {
        let mut client = self.xray_client.write().await;
        if client.is_connected()
            && let Err(e) = client.remove_inbound(inbound_tag).await
        {
            tracing::warn!("outbound_sync: cleanup remove_inbound failed: {e}");
        }
    }
}

fn xray_identity(tag: String, node: &SubscriptionProxy) -> XrayNodeIdentity {
    XrayNodeIdentity::new(
        tag,
        node.protocol_label(),
        node.host().unwrap_or("unknown"),
        node.port().unwrap_or(0),
    )
}

fn xray_identity_from_active(node: &XrayNode) -> XrayNodeIdentity {
    XrayNodeIdentity::new(
        node.tag.clone(),
        node.protocol_label.clone(),
        node.remote_host.clone(),
        node.remote_port,
    )
}

impl XrayNodeConfig {
    fn inbound_tag(&self) -> String {
        format!("in-{}", self.tag)
    }

    fn outbound_tag(&self) -> String {
        format!("out-{}", self.tag)
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
