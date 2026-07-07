//! Round-robin WARP instance balancer.

use crate::models::WarpInstance;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

const GATEWAY_FAILURE_COOLDOWN: Duration = Duration::from_secs(300);

/// Round-robin over healthy WARP instances, with failover marking.
pub struct WarpBalancer {
    instances: Arc<RwLock<Vec<WarpInstance>>>,
    rr: Arc<std::sync::atomic::AtomicUsize>,
    gateway_failed_until: Arc<RwLock<HashMap<u32, Instant>>>,
}

impl WarpBalancer {
    pub fn new(instances: Arc<RwLock<Vec<WarpInstance>>>) -> Self {
        Self {
            instances,
            rr: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            gateway_failed_until: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the list of healthy instances.
    pub async fn healthy_list(&self) -> Vec<WarpInstance> {
        let instances = self.instances.read().await;
        let gateway_failed_until = self.gateway_failed_until.read().await;
        let now = Instant::now();
        instances
            .iter()
            .filter(|i| i.healthy && !cooldown_active(&gateway_failed_until, i.id, now))
            .cloned()
            .collect()
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
        let gateway_failed_until = self.gateway_failed_until.read().await;
        let now = Instant::now();
        instances
            .iter()
            .cloned()
            .map(|mut inst| {
                if cooldown_active(&gateway_failed_until, inst.id, now) {
                    inst.healthy = false;
                }
                inst
            })
            .collect()
    }

    /// Mark an instance as failed.
    pub async fn mark_failed(&self, id: u32) {
        let found = {
            let mut instances = self.instances.write().await;
            if let Some(inst) = instances.iter_mut().find(|i| i.id == id) {
                inst.healthy = false;
                true
            } else {
                false
            }
        };

        if found {
            self.gateway_failed_until
                .write()
                .await
                .insert(id, Instant::now() + GATEWAY_FAILURE_COOLDOWN);
        }
    }
}

fn cooldown_active(cooldowns: &HashMap<u32, Instant>, id: u32, now: Instant) -> bool {
    matches!(cooldowns.get(&id), Some(until) if *until > now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mark_failed_removes_instance_from_healthy_rotation() {
        let instances = Arc::new(RwLock::new(vec![
            WarpInstance::new(1, 40000),
            WarpInstance::new(2, 40001),
        ]));
        let balancer = WarpBalancer::new(instances.clone());

        balancer.mark_failed(1).await;

        let healthy_ids: Vec<u32> = balancer.healthy_list().await.iter().map(|i| i.id).collect();
        assert_eq!(healthy_ids, vec![2]);

        for _ in 0..3 {
            assert_eq!(balancer.next().await.unwrap().id, 2);
        }

        instances.write().await[0].healthy = true;

        let healthy_ids: Vec<u32> = balancer.healthy_list().await.iter().map(|i| i.id).collect();
        assert_eq!(healthy_ids, vec![2]);

        let all = balancer.all_list().await;
        assert!(!all.iter().find(|i| i.id == 1).unwrap().healthy);
    }
}
