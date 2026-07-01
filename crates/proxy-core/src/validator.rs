//! Proxy validation engine: connectivity, latency, anonymity detection.

use crate::models::{Anonymity, Proxy};
use std::sync::Arc;

/// Validates proxies concurrently: connectivity, latency, anonymity.
#[derive(Clone)]
pub struct Validator {
    target_url: String,
    timeout_secs: u64,
    real_ip: Option<String>,
}

impl Validator {
    pub fn new(target_url: &str, timeout_secs: u64) -> Self {
        Self {
            target_url: target_url.to_string(),
            timeout_secs,
            real_ip: None,
        }
    }

    pub fn with_real_ip(mut self, ip: String) -> Self {
        self.real_ip = Some(ip);
        self
    }

    /// Validate a single proxy. Returns `Some(updated Proxy)` if alive, `None` if dead.
    pub async fn validate_one(&self, proxy: &Proxy) -> Option<Proxy> {
        let start = std::time::Instant::now();

        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(proxy.url()).ok()?)
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .no_proxy()
            .build()
            .ok()?;

        let resp = match client.get(&self.target_url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("validate {} failed: {e}", proxy.key());
                return None;
            }
        };

        if resp.status().as_u16() >= 400 {
            return None;
        }

        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        let body_text = resp.text().await.ok();
        let anonymity = self.detect_anonymity(body_text.as_deref(), proxy);

        let mut updated = proxy.clone();
        updated.latency_ms = Some(latency_ms.round());
        updated.anonymity = Some(anonymity);
        updated.success_count += 1;
        updated.last_check = Some(chrono::Utc::now());

        Some(updated)
    }

    /// Validate many proxies concurrently with bounded concurrency.
    pub async fn validate_many(&self, proxies: &[Proxy], concurrency: usize) -> Vec<Proxy> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
        let mut handles = Vec::with_capacity(proxies.len());

        for proxy in proxies {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let validator = self.clone();
            let proxy = proxy.clone();
            handles.push(tokio::spawn(async move {
                let result = validator.validate_one(&proxy).await;
                drop(permit);
                result
            }));
        }

        let mut alive = Vec::new();
        for handle in handles {
            if let Ok(Some(proxy)) = handle.await {
                alive.push(proxy);
            }
        }
        alive
    }

    /// Detect anonymity level from the target URL response body.
    fn detect_anonymity(&self, body: Option<&str>, proxy: &Proxy) -> Anonymity {
        let origin = body
            .and_then(|b| {
                // Try JSON parse first (httpbin returns {"origin": "ip"})
                serde_json::from_str::<serde_json::Value>(b)
                    .ok()
                    .and_then(|v| v.get("origin")?.as_str().map(String::from))
            })
            .unwrap_or_default();

        // If our real IP appears, the proxy is transparent.
        if let Some(ref real_ip) = self.real_ip
            && origin.contains(real_ip)
        {
            return Anonymity::Transparent;
        }

        // If the proxy's own IP appears, it's elite (not forwarding X-Forwarded-For).
        if origin.contains(&proxy.host) {
            return Anonymity::Elite;
        }

        Anonymity::Anonymous
    }
}
