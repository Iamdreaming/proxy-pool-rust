//! Proxy validation engine: connectivity, latency, anonymity detection.

use crate::models::{Anonymity, Proxy};
use crate::pacing::ConnectionPacer;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Default targets used by the multi-target validation matrix.
pub const DEFAULT_MATRIX_TARGETS: &[&str] = &[
    "https://www.cloudflare.com/cdn-cgi/trace",
    "https://httpbin.org/ip",
];

const DEFAULT_MATRIX_TIMEOUT_SECS: u64 = 10;
const MAX_MATRIX_TARGETS: usize = 8;

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

/// Phase timing details for one proxy validation attempt.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProxyCheckTimings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_read_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_ms: Option<f64>,
}

/// Structured result for checking one proxy.
#[derive(Debug, Clone, Serialize)]
pub struct ProxyCheckResult {
    pub alive: bool,
    pub host: String,
    pub port: u16,
    pub protocol: crate::models::Protocol,
    pub target_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anonymity: Option<Anonymity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timings: Option<ProxyCheckTimings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<ProxyCheckErrorType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing)]
    proxy: Option<Proxy>,
}

/// Request body for checking one proxy against several validation targets.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyCheckMatrixRequest {
    pub host: String,
    pub port: u16,
    pub protocol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub targets: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

/// Structured result for a multi-target proxy validation matrix.
#[derive(Debug, Clone, Serialize)]
pub struct ProxyCheckMatrixResult {
    pub host: String,
    pub port: u16,
    pub protocol: crate::models::Protocol,
    pub target_count: usize,
    pub alive_count: usize,
    pub failed_count: usize,
    pub checks: Vec<ProxyCheckResult>,
}

impl ProxyCheckMatrixResult {
    fn from_checks(proxy: &Proxy, checks: Vec<ProxyCheckResult>) -> Self {
        let alive_count = checks.iter().filter(|check| check.alive).count();
        let target_count = checks.len();
        Self {
            host: proxy.host.clone(),
            port: proxy.port,
            protocol: proxy.protocol,
            target_count,
            alive_count,
            failed_count: target_count.saturating_sub(alive_count),
            checks,
        }
    }
}

/// Validation errors for a multi-target proxy matrix request.
#[derive(Debug, thiserror::Error)]
pub enum ProxyCheckMatrixError {
    #[error("{0}")]
    InvalidRequest(String),
}

impl ProxyCheckResult {
    fn success(proxy: Proxy, diagnostics: ProxyCheckDiagnostics) -> Self {
        Self {
            alive: true,
            host: proxy.host.clone(),
            port: proxy.port,
            protocol: proxy.protocol,
            target_url: diagnostics.target_url,
            target_host: diagnostics.target_host,
            latency_ms: proxy.latency_ms,
            anonymity: proxy.anonymity,
            http_status: diagnostics.http_status,
            timings: Some(diagnostics.timings),
            observed_ip: diagnostics.observed_ip,
            observed_country: diagnostics.observed_country,
            error_type: None,
            error: None,
            proxy: Some(proxy),
        }
    }

    fn failure(
        proxy: &Proxy,
        diagnostics: ProxyCheckDiagnostics,
        error_type: ProxyCheckErrorType,
        error: impl Into<String>,
    ) -> Self {
        Self {
            alive: false,
            host: proxy.host.clone(),
            port: proxy.port,
            protocol: proxy.protocol,
            target_url: diagnostics.target_url,
            target_host: diagnostics.target_host,
            latency_ms: None,
            anonymity: None,
            http_status: diagnostics.http_status,
            timings: Some(diagnostics.timings),
            observed_ip: diagnostics.observed_ip,
            observed_country: diagnostics.observed_country,
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

/// Check one proxy against a set of validation targets.
pub async fn check_proxy_matrix(
    request: ProxyCheckMatrixRequest,
) -> Result<ProxyCheckMatrixResult, ProxyCheckMatrixError> {
    let proxy = matrix_request_proxy(&request)?;
    let targets = matrix_targets(request.targets.as_deref())?;
    let timeout_secs = matrix_timeout_secs(request.timeout_secs)?;

    let checks = join_all(targets.into_iter().map(|target| {
        let proxy = proxy.clone();
        async move {
            Validator::new(&target, timeout_secs)
                .check_one(&proxy)
                .await
        }
    }))
    .await;

    Ok(ProxyCheckMatrixResult::from_checks(&proxy, checks))
}

fn matrix_request_proxy(request: &ProxyCheckMatrixRequest) -> Result<Proxy, ProxyCheckMatrixError> {
    let host = request.host.trim();
    if host.is_empty() {
        return Err(ProxyCheckMatrixError::InvalidRequest(
            "host is required".into(),
        ));
    }
    if request.port == 0 {
        return Err(ProxyCheckMatrixError::InvalidRequest(
            "port must be greater than zero".into(),
        ));
    }
    let protocol =
        crate::models::Protocol::from_str_loose(request.protocol.trim()).ok_or_else(|| {
            ProxyCheckMatrixError::InvalidRequest(
                "protocol must be one of: http, https, socks4, socks5".into(),
            )
        })?;
    Ok(Proxy::new(host, request.port, protocol))
}

fn matrix_targets(targets: Option<&[String]>) -> Result<Vec<String>, ProxyCheckMatrixError> {
    let raw_targets: Vec<String> = match targets {
        Some(targets) if !targets.is_empty() => targets.to_vec(),
        _ => DEFAULT_MATRIX_TARGETS
            .iter()
            .map(|target| (*target).to_string())
            .collect(),
    };

    if raw_targets.len() > MAX_MATRIX_TARGETS {
        return Err(ProxyCheckMatrixError::InvalidRequest(format!(
            "targets must contain at most {MAX_MATRIX_TARGETS} entries"
        )));
    }

    raw_targets
        .into_iter()
        .map(|target| normalize_matrix_target(&target))
        .collect()
}

fn normalize_matrix_target(target: &str) -> Result<String, ProxyCheckMatrixError> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return Err(ProxyCheckMatrixError::InvalidRequest(
            "target URL must not be empty".into(),
        ));
    }

    let url = reqwest::Url::parse(trimmed).map_err(|_| {
        ProxyCheckMatrixError::InvalidRequest(format!("invalid target URL: {trimmed}"))
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(ProxyCheckMatrixError::InvalidRequest(format!(
            "target URL must be http(s) with a host: {trimmed}"
        )));
    }
    Ok(url.to_string())
}

fn matrix_timeout_secs(timeout_secs: Option<u64>) -> Result<u64, ProxyCheckMatrixError> {
    let timeout_secs = timeout_secs.unwrap_or(DEFAULT_MATRIX_TIMEOUT_SECS);
    if timeout_secs == 0 || timeout_secs > 60 {
        return Err(ProxyCheckMatrixError::InvalidRequest(
            "timeout_secs must be between 1 and 60".into(),
        ));
    }
    Ok(timeout_secs)
}

#[derive(Debug, Clone)]
struct ProxyCheckDiagnostics {
    target_url: String,
    target_host: Option<String>,
    http_status: Option<u16>,
    timings: ProxyCheckTimings,
    observed_ip: Option<String>,
    observed_country: Option<String>,
}

impl ProxyCheckDiagnostics {
    fn new(target_url: &str) -> Self {
        Self {
            target_url: target_url.to_string(),
            target_host: target_host(target_url),
            http_status: None,
            timings: ProxyCheckTimings::default(),
            observed_ip: None,
            observed_country: None,
        }
    }

    fn with_total(mut self, total: Duration) -> Self {
        self.timings.total_ms = Some(duration_ms(total));
        self
    }

    fn with_request(mut self, request: Duration, total: Duration) -> Self {
        self.timings.request_ms = Some(duration_ms(request));
        self.timings.total_ms = Some(duration_ms(total));
        self
    }

    fn with_response(
        mut self,
        status: u16,
        request: Duration,
        body: Option<Duration>,
        total: Duration,
        observed: ObservedProxyMetadata,
    ) -> Self {
        self.http_status = Some(status);
        self.timings.request_ms = Some(duration_ms(request));
        self.timings.body_read_ms = body.map(duration_ms);
        self.timings.total_ms = Some(duration_ms(total));
        self.observed_ip = observed.ip;
        self.observed_country = observed.country;
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ObservedProxyMetadata {
    ip: Option<String>,
    country: Option<String>,
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

        let start = Instant::now();
        let diagnostics = ProxyCheckDiagnostics::new(&self.target_url);

        let client = reqwest::Client::builder()
            .proxy(match reqwest::Proxy::all(proxy.url()) {
                Ok(proxy) => proxy,
                Err(e) => {
                    return ProxyCheckResult::failure(
                        proxy,
                        diagnostics.with_total(start.elapsed()),
                        ProxyCheckErrorType::InvalidProxyUrl,
                        format!("{e}"),
                    );
                }
            })
            .timeout(Duration::from_secs(self.timeout_secs))
            .connect_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(0)
            .pool_idle_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| format!("{e}"));

        let client = match client {
            Ok(client) => client,
            Err(e) => {
                return ProxyCheckResult::failure(
                    proxy,
                    diagnostics.with_total(start.elapsed()),
                    ProxyCheckErrorType::ClientBuildFailed,
                    e,
                );
            }
        };

        let request_start = Instant::now();
        let resp = match client.get(&self.target_url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("validate {} failed: {e}", proxy.key());
                let kind = if e.is_timeout() {
                    ProxyCheckErrorType::Timeout
                } else {
                    ProxyCheckErrorType::RequestFailed
                };
                return ProxyCheckResult::failure(
                    proxy,
                    diagnostics.with_request(request_start.elapsed(), start.elapsed()),
                    kind,
                    format!("{e}"),
                );
            }
        };
        let request_elapsed = request_start.elapsed();
        let status = resp.status();

        if status.as_u16() >= 400 {
            return ProxyCheckResult::failure(
                proxy,
                diagnostics.with_response(
                    status.as_u16(),
                    request_elapsed,
                    None,
                    start.elapsed(),
                    ObservedProxyMetadata::default(),
                ),
                ProxyCheckErrorType::BadStatus,
                status.to_string(),
            );
        }

        let body_start = Instant::now();
        let body_text = match resp.text().await {
            Ok(text) => text,
            Err(e) => {
                return ProxyCheckResult::failure(
                    proxy,
                    diagnostics.with_response(
                        status.as_u16(),
                        request_elapsed,
                        Some(body_start.elapsed()),
                        start.elapsed(),
                        ObservedProxyMetadata::default(),
                    ),
                    ProxyCheckErrorType::BodyReadFailed,
                    format!("{e}"),
                );
            }
        };
        let body_elapsed = body_start.elapsed();
        let total_elapsed = start.elapsed();
        let latency_ms = total_elapsed.as_secs_f64() * 1000.0;
        let observed = parse_observed_metadata(&body_text);
        let anonymity = self.detect_anonymity(observed.ip.as_deref(), proxy);

        let mut updated = proxy.clone();
        updated.latency_ms = Some(latency_ms.round());
        updated.anonymity = Some(anonymity);
        updated.success_count += 1;
        updated.last_check = Some(chrono::Utc::now());

        ProxyCheckResult::success(
            updated,
            diagnostics.with_response(
                status.as_u16(),
                request_elapsed,
                Some(body_elapsed),
                total_elapsed,
                observed,
            ),
        )
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

    /// Detect anonymity level from the observed origin IP.
    fn detect_anonymity(&self, observed_ip: Option<&str>, proxy: &Proxy) -> Anonymity {
        let origin = observed_ip.unwrap_or_default();
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

fn target_host(target_url: &str) -> Option<String> {
    reqwest::Url::parse(target_url)
        .ok()
        .and_then(|url| url.host_str().map(ToString::to_string))
}

fn duration_ms(duration: Duration) -> f64 {
    (duration.as_secs_f64() * 1000.0 * 100.0).round() / 100.0
}

fn parse_observed_metadata(body: &str) -> ObservedProxyMetadata {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        let ip = value
            .get("origin")
            .and_then(|origin| origin.as_str())
            .and_then(first_non_empty_csv_value);
        return ObservedProxyMetadata { ip, country: None };
    }

    let mut observed = ObservedProxyMetadata::default();
    for line in body.lines() {
        if let Some(ip) = line.strip_prefix("ip=").map(str::trim)
            && !ip.is_empty()
        {
            observed.ip = Some(ip.to_string());
        }
        if let Some(country) = line.strip_prefix("loc=").map(str::trim)
            && !country.is_empty()
        {
            observed.country = Some(country.to_string());
        }
    }
    observed
}

fn first_non_empty_csv_value(value: &str) -> Option<String> {
    value
        .split(',')
        .map(str::trim)
        .find(|part| !part.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Protocol;

    #[test]
    fn proxy_check_failure_serializes_error_type() {
        let proxy = Proxy::new("127.0.0.1", 8080, Protocol::Http);
        let diagnostics = ProxyCheckDiagnostics::new("https://example.com")
            .with_request(Duration::from_millis(12), Duration::from_millis(12));
        let result = ProxyCheckResult::failure(
            &proxy,
            diagnostics,
            ProxyCheckErrorType::RequestFailed,
            "connection refused",
        );

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"alive\":false"));
        assert!(json.contains("\"target_url\":\"https://example.com\""));
        assert!(json.contains("\"target_host\":\"example.com\""));
        assert!(json.contains("\"request_ms\":12.0"));
        assert!(json.contains("\"total_ms\":12.0"));
        assert!(json.contains("\"error_type\":\"request_failed\""));
        assert!(json.contains("connection refused"));
        assert!(result.into_proxy().is_none());
    }

    #[test]
    fn proxy_check_success_carries_validated_proxy() {
        let mut proxy = Proxy::new("1.2.3.4", 8080, Protocol::Http);
        proxy.latency_ms = Some(42.0);
        proxy.anonymity = Some(Anonymity::Elite);

        let diagnostics = ProxyCheckDiagnostics::new("https://www.cloudflare.com/cdn-cgi/trace")
            .with_response(
                200,
                Duration::from_millis(30),
                Some(Duration::from_millis(2)),
                Duration::from_millis(32),
                ObservedProxyMetadata {
                    ip: Some("1.2.3.4".into()),
                    country: Some("US".into()),
                },
            );
        let result = ProxyCheckResult::success(proxy, diagnostics);
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"alive\":true"));
        assert!(json.contains("\"http_status\":200"));
        assert!(json.contains("\"target_host\":\"www.cloudflare.com\""));
        assert!(json.contains("\"request_ms\":30.0"));
        assert!(json.contains("\"body_read_ms\":2.0"));
        assert!(json.contains("\"total_ms\":32.0"));
        assert!(json.contains("\"observed_ip\":\"1.2.3.4\""));
        assert!(json.contains("\"observed_country\":\"US\""));
        assert!(json.contains("\"latency_ms\":42.0"));
        assert!(json.contains("\"anonymity\":\"elite\""));
        assert!(result.into_proxy().is_some());
    }

    #[test]
    fn parse_observed_metadata_supports_cloudflare_trace_and_httpbin_json() {
        let trace = parse_observed_metadata("fl=abc\nip=1.2.3.4\nloc=US\n");
        assert_eq!(trace.ip.as_deref(), Some("1.2.3.4"));
        assert_eq!(trace.country.as_deref(), Some("US"));

        let json = parse_observed_metadata(r#"{"origin":"5.6.7.8, 9.9.9.9"}"#);
        assert_eq!(json.ip.as_deref(), Some("5.6.7.8"));
        assert_eq!(json.country, None);
    }

    #[test]
    fn target_host_returns_none_for_invalid_url() {
        assert_eq!(
            target_host("https://example.com/path").as_deref(),
            Some("example.com")
        );
        assert_eq!(target_host("not a url"), None);
    }

    #[test]
    fn matrix_targets_default_when_not_supplied() {
        let targets = matrix_targets(None).unwrap();
        assert_eq!(
            targets,
            DEFAULT_MATRIX_TARGETS
                .iter()
                .map(|target| (*target).to_string())
                .collect::<Vec<_>>()
        );

        let empty: Vec<String> = vec![];
        let targets = matrix_targets(Some(&empty)).unwrap();
        assert_eq!(
            targets,
            DEFAULT_MATRIX_TARGETS
                .iter()
                .map(|target| (*target).to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn matrix_targets_reject_invalid_entries() {
        let targets = vec!["".to_string()];
        let error = matrix_targets(Some(&targets)).unwrap_err();
        assert_eq!(error.to_string(), "target URL must not be empty");

        let targets = vec!["ftp://example.com/file".to_string()];
        let error = matrix_targets(Some(&targets)).unwrap_err();
        assert!(error.to_string().contains("target URL must be http(s)"));
    }

    #[test]
    fn matrix_request_proxy_validates_identity() {
        let request = ProxyCheckMatrixRequest {
            host: " 1.2.3.4 ".into(),
            port: 8080,
            protocol: "SOCKS5".into(),
            targets: None,
            timeout_secs: None,
        };
        let proxy = matrix_request_proxy(&request).unwrap();
        assert_eq!(proxy.host, "1.2.3.4");
        assert_eq!(proxy.port, 8080);
        assert_eq!(proxy.protocol, Protocol::Socks5);

        let bad = ProxyCheckMatrixRequest {
            protocol: "ssh".into(),
            ..request
        };
        let error = matrix_request_proxy(&bad).unwrap_err();
        assert!(error.to_string().contains("protocol must be one of"));
    }

    #[test]
    fn proxy_check_matrix_result_summarizes_checks() {
        let proxy = Proxy::new("1.2.3.4", 8080, Protocol::Http);
        let success = ProxyCheckResult::success(
            proxy.clone(),
            ProxyCheckDiagnostics::new("https://www.cloudflare.com/cdn-cgi/trace").with_response(
                200,
                Duration::from_millis(20),
                Some(Duration::from_millis(1)),
                Duration::from_millis(21),
                ObservedProxyMetadata {
                    ip: Some("1.2.3.4".into()),
                    country: Some("US".into()),
                },
            ),
        );
        let failure = ProxyCheckResult::failure(
            &proxy,
            ProxyCheckDiagnostics::new("https://httpbin.org/ip")
                .with_request(Duration::from_millis(3), Duration::from_millis(3)),
            ProxyCheckErrorType::RequestFailed,
            "connection refused",
        );

        let matrix = ProxyCheckMatrixResult::from_checks(&proxy, vec![success, failure]);
        assert_eq!(matrix.target_count, 2);
        assert_eq!(matrix.alive_count, 1);
        assert_eq!(matrix.failed_count, 1);

        let json = serde_json::to_string(&matrix).unwrap();
        assert!(json.contains("\"target_count\":2"));
        assert!(json.contains("\"alive_count\":1"));
        assert!(json.contains("\"checks\""));
        assert!(json.contains("\"target_url\":\"https://httpbin.org/ip\""));
    }

    #[test]
    fn detect_anonymity_uses_observed_ip() {
        let proxy = Proxy::new("1.2.3.4", 8080, Protocol::Http);
        let validator = Validator::new("https://example.com", 10).with_real_ip("9.9.9.9".into());

        assert_eq!(
            validator.detect_anonymity(Some("9.9.9.9"), &proxy),
            Anonymity::Transparent
        );
        assert_eq!(
            validator.detect_anonymity(Some("1.2.3.4"), &proxy),
            Anonymity::Elite
        );
    }
}
