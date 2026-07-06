//! Shared xray lifecycle status models.
//!
//! `proxy-xray` owns the state transitions, while API/MCP/server layers only
//! read snapshots from this registry.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Lifecycle state of one encrypted node as it moves through xray activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum XrayNodeLifecycleState {
    /// Node is known but has not started xray activation yet.
    Pending,
    /// Node is currently being configured in xray.
    Activating,
    /// Node has xray config and a local SOCKS5 proxy registered in the pool.
    Active,
    /// Node activation failed and the reason is available in `last_error`.
    Failed,
    /// Node was removed from the active xray set.
    Removed,
}

/// Stable identity for one encrypted node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XrayNodeIdentity {
    /// Unique node tag, normally `{protocol_label}-{host}-{port}`.
    pub tag: String,
    /// Protocol label such as `ss`, `vmess`, or `trojan`.
    pub protocol_label: String,
    /// Remote upstream host from the subscription node.
    pub remote_host: String,
    /// Remote upstream port from the subscription node.
    pub remote_port: u16,
}

impl XrayNodeIdentity {
    /// Create a new xray node identity.
    pub fn new(
        tag: impl Into<String>,
        protocol_label: impl Into<String>,
        remote_host: impl Into<String>,
        remote_port: u16,
    ) -> Self {
        Self {
            tag: tag.into(),
            protocol_label: protocol_label.into(),
            remote_host: remote_host.into(),
            remote_port,
        }
    }
}

/// Operator-visible lifecycle status for one encrypted node.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct XrayNodeStatus {
    /// Unique node tag, normally `{protocol_label}-{host}-{port}`.
    pub tag: String,
    /// Protocol label such as `ss`, `vmess`, or `trojan`.
    pub protocol_label: String,
    /// Remote upstream host from the subscription node.
    pub remote_host: String,
    /// Remote upstream port from the subscription node.
    pub remote_port: u16,
    /// Local SOCKS5 port assigned to this node, if allocation reached that step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_socks5_port: Option<u16>,
    /// Current lifecycle state.
    pub state: XrayNodeLifecycleState,
    /// Most recent activation/removal error, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Last time this lifecycle record changed.
    pub updated_at: DateTime<Utc>,
}

impl XrayNodeStatus {
    fn new(
        identity: &XrayNodeIdentity,
        local_socks5_port: Option<u16>,
        state: XrayNodeLifecycleState,
        last_error: Option<String>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            tag: identity.tag.clone(),
            protocol_label: identity.protocol_label.clone(),
            remote_host: identity.remote_host.clone(),
            remote_port: identity.remote_port,
            local_socks5_port,
            state,
            last_error,
            updated_at,
        }
    }
}

/// xray lifecycle summary returned by API/MCP status surfaces.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct XrayStatusSnapshot {
    /// Whether xray integration is enabled in this process.
    pub enabled: bool,
    /// Number of nodes currently active in xray.
    pub active_nodes: usize,
    /// Number of nodes currently activating.
    pub activating_nodes: usize,
    /// Number of nodes whose latest activation failed.
    pub failed_nodes: usize,
    /// Number of nodes removed from the active xray set.
    pub removed_nodes: usize,
    /// Total records retained in this snapshot.
    pub total_nodes: usize,
    /// Most recently updated lifecycle records, newest first.
    pub recent_nodes: Vec<XrayNodeStatus>,
}

impl XrayStatusSnapshot {
    /// Return a disabled xray snapshot with zero counts.
    pub fn disabled() -> Self {
        Self::default()
    }
}

/// Thread-safe in-memory registry for xray lifecycle records.
#[derive(Debug, Clone, Default)]
pub struct XrayStatusRegistry {
    nodes: Arc<RwLock<HashMap<String, XrayNodeStatus>>>,
}

impl XrayStatusRegistry {
    /// Create an empty xray status registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark a node as pending.
    pub async fn mark_pending(&self, identity: &XrayNodeIdentity) {
        self.update(
            identity,
            None,
            XrayNodeLifecycleState::Pending,
            None,
            Utc::now(),
        )
        .await;
    }

    /// Mark a node as currently activating.
    pub async fn mark_activating(
        &self,
        identity: &XrayNodeIdentity,
        local_socks5_port: Option<u16>,
    ) {
        self.update(
            identity,
            local_socks5_port,
            XrayNodeLifecycleState::Activating,
            None,
            Utc::now(),
        )
        .await;
    }

    /// Mark a node as active.
    pub async fn mark_active(&self, identity: &XrayNodeIdentity, local_socks5_port: u16) {
        self.update(
            identity,
            Some(local_socks5_port),
            XrayNodeLifecycleState::Active,
            None,
            Utc::now(),
        )
        .await;
    }

    /// Mark a node as failed with a human-readable reason.
    pub async fn mark_failed(
        &self,
        identity: &XrayNodeIdentity,
        local_socks5_port: Option<u16>,
        reason: impl Into<String>,
    ) {
        self.update(
            identity,
            local_socks5_port,
            XrayNodeLifecycleState::Failed,
            Some(reason.into()),
            Utc::now(),
        )
        .await;
    }

    /// Mark a node as removed from the active set.
    pub async fn mark_removed(&self, identity: &XrayNodeIdentity, local_socks5_port: Option<u16>) {
        self.update(
            identity,
            local_socks5_port,
            XrayNodeLifecycleState::Removed,
            None,
            Utc::now(),
        )
        .await;
    }

    /// Return a snapshot with the most recently updated records first.
    pub async fn snapshot(&self, enabled: bool, recent_limit: usize) -> XrayStatusSnapshot {
        if !enabled {
            return XrayStatusSnapshot::disabled();
        }

        let nodes = self.nodes.read().await;
        let mut recent_nodes: Vec<XrayNodeStatus> = nodes.values().cloned().collect();
        recent_nodes.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.tag.cmp(&b.tag))
        });
        if recent_limit > 0 {
            recent_nodes.truncate(recent_limit);
        }

        XrayStatusSnapshot {
            enabled,
            active_nodes: nodes
                .values()
                .filter(|n| n.state == XrayNodeLifecycleState::Active)
                .count(),
            activating_nodes: nodes
                .values()
                .filter(|n| n.state == XrayNodeLifecycleState::Activating)
                .count(),
            failed_nodes: nodes
                .values()
                .filter(|n| n.state == XrayNodeLifecycleState::Failed)
                .count(),
            removed_nodes: nodes
                .values()
                .filter(|n| n.state == XrayNodeLifecycleState::Removed)
                .count(),
            total_nodes: nodes.len(),
            recent_nodes,
        }
    }

    async fn update(
        &self,
        identity: &XrayNodeIdentity,
        local_socks5_port: Option<u16>,
        state: XrayNodeLifecycleState,
        last_error: Option<String>,
        updated_at: DateTime<Utc>,
    ) {
        let mut nodes = self.nodes.write().await;
        nodes.insert(
            identity.tag.clone(),
            XrayNodeStatus::new(identity, local_socks5_port, state, last_error, updated_at),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(tag: &str) -> XrayNodeIdentity {
        XrayNodeIdentity::new(tag, "ss", "example.com", 8388)
    }

    #[tokio::test]
    async fn registry_snapshot_counts_lifecycle_states() {
        let registry = XrayStatusRegistry::new();
        let active = identity("ss-active-8388");
        let failed = identity("ss-failed-8389");
        let removed = identity("ss-removed-8390");

        registry.mark_active(&active, 20000).await;
        registry
            .mark_failed(&failed, Some(20001), "xray add_outbound failed")
            .await;
        registry.mark_removed(&removed, Some(20002)).await;

        let snapshot = registry.snapshot(true, 10).await;

        assert!(snapshot.enabled);
        assert_eq!(snapshot.active_nodes, 1);
        assert_eq!(snapshot.failed_nodes, 1);
        assert_eq!(snapshot.removed_nodes, 1);
        assert_eq!(snapshot.total_nodes, 3);
        assert_eq!(snapshot.recent_nodes.len(), 3);
    }

    #[tokio::test]
    async fn failed_state_preserves_reason_and_port() {
        let registry = XrayStatusRegistry::new();
        let node = identity("ss-failed-8388");

        registry
            .mark_failed(&node, Some(20000), "port allocation exhausted")
            .await;

        let snapshot = registry.snapshot(true, 10).await;
        let failed = snapshot.recent_nodes.first().expect("missing failed node");

        assert_eq!(failed.state, XrayNodeLifecycleState::Failed);
        assert_eq!(failed.local_socks5_port, Some(20000));
        assert_eq!(
            failed.last_error.as_deref(),
            Some("port allocation exhausted")
        );
    }

    #[tokio::test]
    async fn disabled_snapshot_hides_retained_records() {
        let registry = XrayStatusRegistry::new();
        registry
            .mark_active(&identity("ss-active-8388"), 20000)
            .await;

        let snapshot = registry.snapshot(false, 10).await;

        assert!(!snapshot.enabled);
        assert_eq!(snapshot.active_nodes, 0);
        assert_eq!(snapshot.total_nodes, 0);
        assert!(snapshot.recent_nodes.is_empty());
    }

    #[tokio::test]
    async fn snapshot_respects_recent_limit() {
        let registry = XrayStatusRegistry::new();
        registry.mark_active(&identity("ss-one-8388"), 20000).await;
        registry.mark_active(&identity("ss-two-8389"), 20001).await;

        let snapshot = registry.snapshot(true, 1).await;

        assert_eq!(snapshot.total_nodes, 2);
        assert_eq!(snapshot.recent_nodes.len(), 1);
    }
}
