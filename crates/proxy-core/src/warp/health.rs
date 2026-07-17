//! WARP instance health checker.

use crate::config::WarpSettings;
use crate::models::WarpInstance;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Periodically probes each WARP instance and maintains healthy/fail_streak.
pub struct WarpHealthChecker {
    instances: Arc<RwLock<Vec<WarpInstance>>>,
    settings: WarpSettings,
}

impl WarpHealthChecker {
    pub fn new(instances: Arc<RwLock<Vec<WarpInstance>>>, settings: WarpSettings) -> Self {
        Self {
            instances,
            settings,
        }
    }

    /// Perform a single health check pass.
    pub async fn check_once(&self) {
        // Read instances and probe each one
        let results: Vec<(u32, bool)> = {
            let instances = self.instances.read().await;
            let mut results = Vec::new();
            for inst in instances.iter() {
                let ok = self.probe(&inst.socks5_host, inst.socks5_port).await;
                results.push((inst.id, ok));
            }
            results
        };

        // Write back health status
        let mut instances = self.instances.write().await;
        for (id, ok) in results {
            if let Some(i) = instances.iter_mut().find(|i| i.id == id) {
                if ok {
                    i.healthy = true;
                    i.fail_streak = 0;
                } else {
                    i.fail_streak += 1;
                    if i.fail_streak >= self.settings.health_check_fail_threshold {
                        i.healthy = false;
                    }
                }
            }
        }
    }

    /// Probe a WARP instance via its SOCKS5 port.
    async fn probe(&self, host: &str, port: u16) -> bool {
        let url = &self.settings.health_check_url;
        let proxy_url = format!("socks5://{host}:{port}");

        let client = match reqwest::Client::builder()
            .proxy(match reqwest::Proxy::all(&proxy_url) {
                Ok(p) => p,
                Err(_) => return false,
            })
            .timeout(std::time::Duration::from_secs(
                self.settings.health_check_timeout_sec,
            ))
            .no_proxy()
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };

        match client.get(url).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Run the health check loop.
    pub async fn run(self: Arc<Self>) {
        let interval = std::time::Duration::from_secs(self.settings.health_check_interval_sec);
        loop {
            self.check_once().await;
            tokio::time::sleep(interval).await;
        }
    }
}
