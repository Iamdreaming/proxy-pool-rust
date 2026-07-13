//! Background loop that reads pending encrypted nodes from Redis,
//! configures xray-core outbounds, and creates active Proxy entries.
//!
//! The sync loop pauses when the xray gRPC connection is lost and
//! resumes automatically on reconnection.

use crate::config_gen::{ConfigGenerator, XrayNodeConfig, is_xray_activatable};
use crate::models::{SyncStats, XrayNode};
use crate::port_manager::PortManager;
use crate::xray_client::XrayClient;
use proxy_core::config::{PoolSettings, XraySettings};
use proxy_core::models::{EncryptedProxyState, Protocol, Proxy};
use proxy_core::store::ProxyStore;
use proxy_core::validator::{ValidationTarget, Validator};
use proxy_core::xray_status::{XrayNodeIdentity, XrayStatusRegistry};
use proxy_sub::models::SubscriptionProxy;
use proxy_sub::pending::PendingStore;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::sync::watch;

/// Admission-validation plan for xray nodes before they become routeable.
#[derive(Debug, Clone)]
pub struct XrayValidationPlan {
    /// Targets that every candidate must pass.
    pub targets: Vec<ValidationTarget>,
    /// Request timeout for each validation target.
    pub timeout_secs: u64,
    /// Maximum candidate validation attempts per sync cycle.
    pub attempt_limit_per_cycle: usize,
    /// Cooldown applied after a candidate fails validation.
    pub failure_cooldown: Duration,
}

impl XrayValidationPlan {
    /// Build a validation plan from xray settings, falling back to pool targets.
    pub fn from_settings(xray: &XraySettings, pool: &PoolSettings) -> Self {
        let target_configs = if xray.validate_targets.is_empty() {
            pool.effective_validate_targets()
        } else {
            xray.validate_targets.clone()
        };
        let targets = target_configs
            .into_iter()
            .map(ValidationTarget::from)
            .collect();

        Self {
            targets,
            timeout_secs: xray
                .validate_timeout_sec
                .unwrap_or(pool.validate_timeout_sec),
            attempt_limit_per_cycle: xray.validation_attempt_limit_per_cycle,
            failure_cooldown: Duration::from_secs(xray.validation_failure_cooldown_sec),
        }
    }
}

/// Runtime options for `OutboundSync`.
pub struct OutboundSyncOptions {
    /// Xray sync settings from service configuration.
    pub config: XraySettings,
    /// Admission-validation plan for candidate nodes.
    pub validation: XrayValidationPlan,
    /// Watch receiver for xray gRPC connection state.
    pub connected_rx: watch::Receiver<bool>,
    /// Shared lifecycle status registry.
    pub status_registry: XrayStatusRegistry,
}

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
    validation_failed_until: Arc<RwLock<HashMap<String, Instant>>>,
    status_registry: XrayStatusRegistry,
    config: XraySettings,
    validation: XrayValidationPlan,
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
        options: OutboundSyncOptions,
    ) -> Self {
        Self {
            pending_store,
            proxy_store,
            xray_client,
            port_manager,
            active_nodes: Arc::new(RwLock::new(HashMap::new())),
            validation_failed_until: Arc::new(RwLock::new(HashMap::new())),
            status_registry: options.status_registry,
            config: options.config,
            validation: options.validation,
            connected_rx: options.connected_rx,
        }
    }

    /// Run a single sync cycle.
    ///
    /// Iterates over encrypted protocol labels (ss, vmess, trojan, vless), reads
    /// pending nodes from Redis, and activates any that are not yet in the
    /// active set.
    pub async fn sync_once(&self) -> SyncStats {
        let mut stats = SyncStats::default();
        let labels = ["ss", "vmess", "trojan", "vless"];
        let mut validation_attempts = 0usize;

        'labels: for label in labels {
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

                if self.validation_cooldown_active(&tag).await {
                    continue;
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

                // Skip nodes xray cannot build an outbound for: Basic/Unknown
                // (should not appear here) and Shadowsocks nodes using a legacy
                // cipher xray-core rejects (aes-*-cfb, rc4-md5, ...). Filtering
                // here avoids spending a port, a validation attempt, and an
                // `xray api ado` round-trip on a node that can never activate.
                if !is_xray_activatable(node) {
                    tracing::debug!(
                        tag = %tag,
                        "outbound_sync: skipping node xray cannot activate"
                    );
                    continue;
                }

                if validation_attempts >= self.validation.attempt_limit_per_cycle {
                    tracing::warn!(
                        "outbound_sync: validation_attempt_limit_per_cycle ({}) reached",
                        self.validation.attempt_limit_per_cycle
                    );
                    break 'labels;
                }
                validation_attempts += 1;

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

                let validated_proxy = match self.validate_candidate(&proxy).await {
                    Some(proxy) => proxy,
                    None => {
                        tracing::warn!(
                            "outbound_sync: validation failed for {} -> local port {}",
                            node_config.tag,
                            local_port
                        );
                        self.cleanup_xray_config(&node_config).await;
                        self.port_manager.release(local_port).await;
                        self.status_registry
                            .mark_failed(&identity, Some(local_port), "xray validation failed")
                            .await;
                        self.mark_validation_failed(tag.clone()).await;
                        stats.failed += 1;
                        continue;
                    }
                };

                tracing::info!(
                    "outbound_sync: activated {} -> local port {} after validation",
                    node_config.tag,
                    local_port
                );

                if let Err(e) = self.proxy_store.add(&validated_proxy).await {
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

                    // Remove the pool Proxy entry for this node's local port so a
                    // later port reuse cannot route traffic through the wrong
                    // (torn-down) node (B2).
                    let pool_entry =
                        Proxy::new("127.0.0.1", node.local_socks5_port, Protocol::Socks5);
                    if let Err(e) = self.proxy_store.remove(&pool_entry).await {
                        tracing::warn!(
                            "outbound_sync: failed to remove stale pool entry for port {}: {e}",
                            node.local_socks5_port
                        );
                    }

                    // Remove from xray via gRPC.
                    // Use write lock because remove_inbound takes &mut self.
                    let mut client = self.xray_client.write().await;
                    if client.is_connected() {
                        if let Err(e) = client.remove_routing_rule(&node.routing_rule_tag()).await {
                            tracing::warn!("outbound_sync: remove_routing_rule failed: {e}");
                        }
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
            Some(Ok(())) => {}
            Some(Err(e)) => {
                self.cleanup_inbound(&node_config.inbound_tag()).await;
                return Err(format!("add_outbound failed: {e}"));
            }
            None => {
                self.cleanup_inbound(&node_config.inbound_tag()).await;
                return Err("xray gRPC client disconnected after add_inbound".into());
            }
        }

        // Install the routing rule that binds this node's inbound to its
        // outbound. Without it, xray routes the inbound to the first outbound
        // (bootstrap `direct`) and the encrypted node is bypassed entirely.
        let routing_result = {
            let client = self.xray_client.read().await;
            if !client.is_connected() {
                None
            } else {
                // Best-effort clear any stale rule with the same ruleTag first,
                // so a leftover orphan (e.g. from an earlier incomplete cleanup)
                // cannot make AddRule fail with "duplicate ruleTag".
                let _ = client
                    .remove_routing_rule(&node_config.routing_rule_tag())
                    .await;
                Some(
                    client
                        .add_routing_rule(&node_config.routing_rule_json)
                        .await,
                )
            }
        };

        match routing_result {
            Some(Ok(())) => Ok(()),
            Some(Err(e)) => {
                self.cleanup_xray_config(node_config).await;
                Err(format!("add_routing_rule failed: {e}"))
            }
            None => {
                self.cleanup_xray_config(node_config).await;
                Err("xray gRPC client disconnected after add_outbound".into())
            }
        }
    }

    async fn cleanup_xray_config(&self, node_config: &XrayNodeConfig) {
        let mut client = self.xray_client.write().await;
        if !client.is_connected() {
            return;
        }
        if let Err(e) = client
            .remove_routing_rule(&node_config.routing_rule_tag())
            .await
        {
            tracing::warn!("outbound_sync: cleanup remove_routing_rule failed: {e}");
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

    async fn validate_candidate(&self, proxy: &Proxy) -> Option<Proxy> {
        let Some(first_target) = self.validation.targets.first() else {
            tracing::warn!("outbound_sync: no xray validation targets configured");
            return None;
        };
        Validator::new(&first_target.url, self.validation.timeout_secs)
            .validate_one_against_targets(proxy, &self.validation.targets)
            .await
    }

    async fn validation_cooldown_active(&self, tag: &str) -> bool {
        let now = Instant::now();
        let cooldowns = self.validation_failed_until.read().await;
        matches!(cooldowns.get(tag), Some(until) if *until > now)
    }

    async fn mark_validation_failed(&self, tag: String) {
        self.validation_failed_until
            .write()
            .await
            .insert(tag, Instant::now() + self.validation.failure_cooldown);
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

    fn routing_rule_tag(&self) -> String {
        crate::config_gen::routing_rule_tag(&self.inbound_tag())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxy_core::config::ValidationTargetConfig;

    #[test]
    fn test_sync_stats_default() {
        let stats = SyncStats::default();
        assert_eq!(stats.added, 0);
        assert_eq!(stats.removed, 0);
        assert_eq!(stats.failed, 0);
        assert_eq!(stats.total_active, 0);
    }

    #[test]
    fn validation_plan_falls_back_to_pool_targets() {
        let xray = XraySettings::default();
        let pool = PoolSettings {
            validate_timeout_sec: 11,
            validate_targets: vec![ValidationTargetConfig {
                url: "https://pool.example/check".into(),
                expected_statuses: vec![204],
            }],
            ..PoolSettings::default()
        };

        let plan = XrayValidationPlan::from_settings(&xray, &pool);

        assert_eq!(plan.timeout_secs, 11);
        assert_eq!(plan.attempt_limit_per_cycle, 50);
        assert_eq!(plan.failure_cooldown, Duration::from_secs(600));
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].url, "https://pool.example/check");
        assert_eq!(plan.targets[0].expected_statuses, vec![204]);
    }

    #[test]
    fn validation_plan_prefers_xray_targets_and_timeout() {
        let xray = XraySettings {
            validate_timeout_sec: Some(5),
            validate_targets: vec![ValidationTargetConfig {
                url: "https://xray.example/check".into(),
                expected_statuses: vec![200, 204],
            }],
            ..XraySettings::default()
        };
        let pool = PoolSettings {
            validate_timeout_sec: 11,
            validate_targets: vec![ValidationTargetConfig::from_url(
                "https://pool.example/check",
            )],
            ..PoolSettings::default()
        };

        let plan = XrayValidationPlan::from_settings(&xray, &pool);

        assert_eq!(plan.timeout_secs, 5);
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].url, "https://xray.example/check");
        assert_eq!(plan.targets[0].expected_statuses, vec![200, 204]);
    }
}
