//! Round-robin WARP instance balancer.

use crate::models::WarpInstance;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Round-robin over healthy WARP instances, with failover marking.
pub struct WarpBalancer {
    instances: Arc<RwLock<Vec<WarpInstance>>>,
    rr: Arc<std::sync::atomic::AtomicUsize>,
}

impl WarpBalancer {
    pub fn new(instances: Arc<RwLock<Vec<WarpInstance>>>) -> Self {
        Self {
            instances,
            rr: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// Get the list of healthy instances.
    pub async fn healthy_list(&self) -> Vec<WarpInstance> {
        let instances = self.instances.read().await;
        instances.iter().filter(|i| i.healthy).cloned().collect()
    }

    /// Select the next healthy instance (round-robin).
    pub async fn next(&self) -> Option<WarpInstance> {
        let healthy = self.healthy_list().await;
        if healthy.is_empty() {
            return None;
        }
        let idx = self.rr.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % healthy.len();
        Some(healthy[idx].clone())
    }

    /// Get the list of all instances (healthy and unhealthy).
    pub async fn all_list(&self) -> Vec<WarpInstance> {
        let instances = self.instances.read().await;
        instances.iter().cloned().collect()
    }

    /// Mark an instance as failed.
    pub async fn mark_failed(&self, id: u32) {
        let mut instances = self.instances.write().await;
        if let Some(inst) = instances.iter_mut().find(|i| i.id == id) {
            inst.healthy = false;
        }
    }
}
