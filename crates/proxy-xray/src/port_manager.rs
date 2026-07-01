//! Manages allocation of local ports for xray SOCKS5 inbounds.
//!
//! Tracks which ports are in use via an in-memory HashSet.
//! Default range: 20000-29999 (configurable).

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Manages allocation of local ports for xray SOCKS5 inbounds.
pub struct PortManager {
    allocated: Arc<RwLock<HashSet<u16>>>,
    range_start: u16,
    range_end: u16,
}

impl PortManager {
    /// Create a new `PortManager` with the given port range (inclusive on both ends).
    pub fn new(range_start: u16, range_end: u16) -> Self {
        Self {
            allocated: Arc::new(RwLock::new(HashSet::new())),
            range_start,
            range_end,
        }
    }

    /// Allocate the next available port. Returns `None` if range exhausted.
    pub async fn allocate(&self) -> Option<u16> {
        let mut allocated = self.allocated.write().await;
        for port in self.range_start..=self.range_end {
            if !allocated.contains(&port) {
                allocated.insert(port);
                return Some(port);
            }
        }
        None
    }

    /// Release a previously allocated port back to the pool.
    pub async fn release(&self, port: u16) {
        let mut allocated = self.allocated.write().await;
        allocated.remove(&port);
    }

    /// Check if a specific port is currently allocated.
    pub async fn is_allocated(&self, port: u16) -> bool {
        let allocated = self.allocated.read().await;
        allocated.contains(&port)
    }

    /// Number of ports currently in use.
    pub async fn used_count(&self) -> usize {
        let allocated = self.allocated.read().await;
        allocated.len()
    }

    /// Re-allocate a specific port (used during restart re-sync).
    ///
    /// Returns `true` if the port was successfully claimed (wasn't already allocated).
    pub async fn claim(&self, port: u16) -> bool {
        let mut allocated = self.allocated.write().await;
        allocated.insert(port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_allocate_sequential() {
        let pm = PortManager::new(20000, 20010);
        let p1 = pm.allocate().await.unwrap();
        let p2 = pm.allocate().await.unwrap();
        assert_eq!(p1, 20000);
        assert_eq!(p2, 20001);
    }

    #[tokio::test]
    async fn test_allocate_release_reuse() {
        let pm = PortManager::new(20000, 20001);
        let p1 = pm.allocate().await.unwrap();
        assert_eq!(p1, 20000);
        pm.release(p1).await;
        let p2 = pm.allocate().await.unwrap();
        assert_eq!(p2, 20000); // re-uses released port
    }

    #[tokio::test]
    async fn test_allocate_exhaustion() {
        let pm = PortManager::new(20000, 20000);
        let p1 = pm.allocate().await;
        assert_eq!(p1, Some(20000));
        let p2 = pm.allocate().await;
        assert_eq!(p2, None); // exhausted
    }

    #[tokio::test]
    async fn test_is_allocated() {
        let pm = PortManager::new(20000, 20010);
        assert!(!pm.is_allocated(20000).await);
        pm.allocate().await;
        assert!(pm.is_allocated(20000).await);
    }

    #[tokio::test]
    async fn test_used_count() {
        let pm = PortManager::new(20000, 20010);
        assert_eq!(pm.used_count().await, 0);
        pm.allocate().await;
        pm.allocate().await;
        assert_eq!(pm.used_count().await, 2);
    }

    #[tokio::test]
    async fn test_claim() {
        let pm = PortManager::new(20000, 20010);
        assert!(pm.claim(20005).await);
        assert!(!pm.claim(20005).await); // already claimed
    }
}
