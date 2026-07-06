//! Proxy validation engine: connectivity, latency, anonymity detection.

use crate::models::{Anonymity, Proxy};
use crate::pacing::ConnectionPacer;
use serde::Serialize;
use std::sync::Arc;

/// Stable validation failure category for API/MCP clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyCheckErrorType {
    InvalidProxyUrl,
    ClientBuildFailed,
    Timeout,
    RequestFailed,
    BadStatus,
    BodyReadFailed,
}

/// Structured result for checking one proxy.
#[derive(Debug, Clone, Serialize)]
pub struct ProxyCheckResult {
    pub alive: bool,
    pub host: String,
    pub port: u16,
    pub protocol: crate::models::Protocol,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anonymity: Option<Anonymity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<ProxyCheckErrorType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing)]
    proxy: Option<Proxy>,
}

impl ProxyCheckResult {
    fn success(proxy: Proxy) -> Self {
        Self {
            alive: true,
            host: proxy.host.clone(),
            port: proxy.port,
            protocol: proxy.protocol,
            latency_ms: proxy.latency_ms,
            anonymity: proxy.anonymity,
            error_type: None,
            error: None,
            proxy: Some(proxy),
        }
    }

    fn failure(proxy: &Proxy, error_type: ProxyCheckErrorType, error: impl Into<String>) -> Self {
        Self {
            alive: false,
            host: proxy.host.clone(),
            port: proxy.port,
            protocol: proxy.protocol,
            latency_ms: None,
            anonymity: None,
            error_type: Some(error_type),
            error: Some(error.into()),
            proxy: None,
        }
    }

    /// Return the validated proxy when the check succeeded.
    pub fn into_proxy(self) -> Option<Proxy> {
        self.proxy
    }
}

/// Validates proxies concurrently: connectivity, latency, anonymity.
#[derive(Clone)]
pub struct Validator {
    target_url: String,
    timeout_secs: u64,
    real_ip: Option<String>,
    /// Optional connection rate pacer.
    pacer: Option<Arc<ConnectionPacer>>,
}

impl Validator {
    pub fn new(target_url: &str, timeout_secs: u64) -> Self {
        Self {
            target_url: target_url.to_string(),
            timeout_secs,
            real_ip: None,
            pacer: None,
        }
    }

    pub fn with_real_ip(mut self, ip: String) -> Self {
        self.real_ip = Some(ip);
        self
    }

    /// Attach a connection rate pacer to this validator.
    pub fn with_pacer(mut self, pacer: Arc<ConnectionPacer>) -> Self {
        self.pacer = Some(pacer);
        self
    }

    /// Validate a single proxy. Returns `Some(updated Proxy)` if alive, `None` if dead.
    pub async fn validate_one(&self, proxy: &Proxy) -> Option<Proxy> {
        self.check_one(proxy).await.into_proxy()
    }

    /// Check a single proxy and return a structured validation result.
    pub async fn check_one(&self, proxy: &Proxy) -> ProxyCheckResult {
        // Rate-limit if pacer is configured
        if let Some(ref pacer) = self.pacer {
            pacer.acquire().await;
        }

        let start = std::time::Instant::now();

        let client = reqwest::Client::builder()
            .proxy(match reqwest::Proxy::all(proxy.url()) {
                Ok(proxy) => proxy,
                Err(e) => {
                    return ProxyCheckResult::failure(
                        proxy,
                        ProxyCheckErrorType::InvalidProxyUrl,
                        format!("{e}"),
                    );
                }
            })
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .connect_timeout(std::time::Duration::from_secs(10))
            .pool_max_idle_per_host(0)
            .pool_idle_timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| format!("{e}"));

        let client = match client {
            Ok(client) => client,
            Err(e) => {
                return ProxyCheckResult::failure(proxy, ProxyCheckErrorType::ClientBuildFailed, e);
            }
        };

        let resp = match client.get(&self.target_url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("validate {} failed: {e}", proxy.key());
                let kind = if e.is_timeout() {
                    ProxyCheckErrorType::Timeout
                } else {
                    ProxyCheckErrorType::RequestFailed
                };
                return ProxyCheckResult::failure(proxy, kind, format!("{e}"));
            }
        };

        if resp.status().as_u16() >= 400 {
            return ProxyCheckResult::failure(
                proxy,
                ProxyCheckErrorType::BadStatus,
                resp.status().to_string(),
            );
        }

        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        let body_text = match resp.text().await {
            Ok(text) => text,
            Err(e) => {
                return ProxyCheckResult::failure(
                    proxy,
                    ProxyCheckErrorType::BodyReadFailed,
                    format!("{e}"),
                );
            }
        };
        let anonymity = self.detect_anonymity(Some(&body_text), proxy);

        let mut updated = proxy.clone();
        updated.latency_ms = Some(latency_ms.round());
        updated.anonymity = Some(anonymity);
        updated.success_count += 1;
        updated.last_check = Some(chrono::Utc::now());

        ProxyCheckResult::success(updated)
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
                    .or_else(|| {
                        // Cloudflare cdn-cgi/trace returns "ip=1.2.3.4\n..."
                        b.lines()
                            .find(|l| l.starts_with("ip="))
                            .map(|l| l.trim_start_matches("ip=").to_string())
                    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Protocol;

    #[test]
    fn proxy_check_failure_serializes_error_type() {
        let proxy = Proxy::new("127.0.0.1", 8080, Protocol::Http);
        let result = ProxyCheckResult::failure(
            &proxy,
            ProxyCheckErrorType::RequestFailed,
            "connection refused",
        );

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"alive\":false"));
        assert!(json.contains("\"error_type\":\"request_failed\""));
        assert!(json.contains("connection refused"));
        assert!(result.into_proxy().is_none());
    }

    #[test]
    fn proxy_check_success_carries_validated_proxy() {
        let mut proxy = Proxy::new("1.2.3.4", 8080, Protocol::Http);
        proxy.latency_ms = Some(42.0);
        proxy.anonymity = Some(Anonymity::Elite);

        let result = ProxyCheckResult::success(proxy);
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"alive\":true"));
        assert!(json.contains("\"latency_ms\":42.0"));
        assert!(json.contains("\"anonymity\":\"elite\""));
        assert!(result.into_proxy().is_some());
    }

    #[test]
    fn detect_anonymity_uses_real_ip_and_proxy_host() {
        let proxy = Proxy::new("1.2.3.4", 8080, Protocol::Http);
        let validator = Validator::new("https://example.com", 10).with_real_ip("9.9.9.9".into());

        assert_eq!(
            validator.detect_anonymity(Some("ip=9.9.9.9\n"), &proxy),
            Anonymity::Transparent
        );
        assert_eq!(
            validator.detect_anonymity(Some("{\"origin\":\"1.2.3.4\"}"), &proxy),
            Anonymity::Elite
        );
    }
}
