//! Proxy validation engine: connectivity, latency, anonymity detection.

use crate::models::{Anonymity, Proxy};
use crate::pacing::ConnectionPacer;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Default targets used by the multi-target validation matrix.
///
/// Both are highly-available generate_204/trace endpoints; httpbin.org was
/// dropped because its frequent rate-limiting produced false negatives.
pub const DEFAULT_MATRIX_TARGETS: &[&str] = &[
    "https://www.cloudflare.com/cdn-cgi/trace",
    "https://www.gstatic.com/generate_204",
];

const DEFAULT_MATRIX_TIMEOUT_SECS: u64 = 10;
const MAX_MATRIX_TARGETS: usize = 8;

/// One validation target plus optional accepted HTTP status codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationTarget {
    /// Target URL to request through the proxy.
    pub url: String,
    /// Accepted status codes. Empty means any status below 400 is accepted.
    pub expected_statuses: Vec<u16>,
}

impl ValidationTarget {
    /// Build a target that accepts any HTTP status below 400.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            expected_statuses: Vec::new(),
        }
    }

    /// Build a target that accepts only the supplied HTTP status codes.
    pub fn with_expected_statuses(url: impl Into<String>, expected_statuses: Vec<u16>) -> Self {
        Self {
            url: url.into(),
            expected_statuses,
        }
    }
}

impl From<crate::config::ValidationTargetConfig> for ValidationTarget {
    fn from(config: crate::config::ValidationTargetConfig) -> Self {
        Self::with_expected_statuses(config.url, config.expected_statuses)
    }
}

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

/// Request target for checking one proxy against one validation target.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ProxyCheckMatrixTarget {
    /// Backward-compatible target URL string.
    Url(String),
    /// Structured target with explicit successful HTTP status codes.
    Structured {
        url: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        expected_statuses: Vec<u16>,
    },
}

impl ProxyCheckMatrixTarget {
    fn into_validation_target(self) -> Result<ValidationTarget, ProxyCheckMatrixError> {
        let (url, expected_statuses) = match self {
            Self::Url(url) => (url, Vec::new()),
            Self::Structured {
                url,
                expected_statuses,
            } => (url, expected_statuses),
        };
        let url = normalize_matrix_target_url(&url)?;
        Ok(ValidationTarget::with_expected_statuses(
            url,
            expected_statuses,
        ))
    }
}

/// Request body for checking one proxy against several validation targets.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyCheckMatrixRequest {
    pub host: String,
    pub port: u16,
    pub protocol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub targets: Option<Vec<ProxyCheckMatrixTarget>>,
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
            Validator::new(&target.url, timeout_secs)
                .with_expected_statuses(target.expected_statuses)
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

fn matrix_targets(
    targets: Option<&[ProxyCheckMatrixTarget]>,
) -> Result<Vec<ValidationTarget>, ProxyCheckMatrixError> {
    let raw_targets: Vec<ProxyCheckMatrixTarget> = match targets {
        Some(targets) if !targets.is_empty() => targets.to_vec(),
        _ => DEFAULT_MATRIX_TARGETS
            .iter()
            .map(|target| ProxyCheckMatrixTarget::Url((*target).to_string()))
            .collect(),
    };

    if raw_targets.len() > MAX_MATRIX_TARGETS {
        return Err(ProxyCheckMatrixError::InvalidRequest(format!(
            "targets must contain at most {MAX_MATRIX_TARGETS} entries"
        )));
    }

    raw_targets
        .into_iter()
        .map(ProxyCheckMatrixTarget::into_validation_target)
        .collect()
}

fn normalize_matrix_target_url(target: &str) -> Result<String, ProxyCheckMatrixError> {
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
    expected_statuses: Vec<u16>,
    timeout_secs: u64,
    real_ip: Option<String>,
    /// Optional connection rate pacer.
    pacer: Option<Arc<ConnectionPacer>>,
}

impl Validator {
    pub fn new(target_url: &str, timeout_secs: u64) -> Self {
        Self {
            target_url: target_url.to_string(),
            expected_statuses: Vec::new(),
            timeout_secs,
            real_ip: None,
            pacer: None,
        }
    }

    pub fn with_real_ip(mut self, ip: String) -> Self {
        self.real_ip = Some(ip);
        self
    }

    /// Accept only the configured HTTP status codes as validation success.
    pub fn with_expected_statuses(mut self, expected_statuses: Vec<u16>) -> Self {
        self.expected_statuses = expected_statuses;
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

    /// Validate a single proxy against every target URL. All targets must pass.
    pub async fn validate_one_against_targets(
        &self,
        proxy: &Proxy,
        targets: &[ValidationTarget],
    ) -> Option<Proxy> {
        self.validate_one_with_admission(proxy, targets, TargetAdmission::Strict)
            .await
    }

    /// Validate a single proxy against every target URL under an admission mode.
    ///
    /// - `Strict`: every target must pass (short-circuits on first failure).
    /// - `Quorum`: the proxy is alive if at least one target passes; the
    ///   remaining targets are still probed so a partial result is possible.
    pub async fn validate_one_with_admission(
        &self,
        proxy: &Proxy,
        targets: &[ValidationTarget],
        mode: TargetAdmission,
    ) -> Option<Proxy> {
        if targets.is_empty() {
            return self.validate_one(proxy).await;
        }

        let mut checks = Vec::with_capacity(targets.len());
        for target in targets {
            let validator = Self {
                target_url: target.url.clone(),
                expected_statuses: target.expected_statuses.clone(),
                timeout_secs: self.timeout_secs,
                real_ip: self.real_ip.clone(),
                pacer: self.pacer.clone(),
            };
            let result = validator.validate_one(proxy).await;
            let failed = result.is_none();
            checks.push(result);
            // Strict mode can stop at the first failure; quorum needs every
            // probe to know whether any target passed.
            if failed && mode == TargetAdmission::Strict {
                break;
            }
        }
        match mode {
            TargetAdmission::Strict => strict_target_admission_result(proxy, checks),
            TargetAdmission::Quorum => quorum_target_admission_result(proxy, checks),
        }
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
            .proxy(match reqwest::Proxy::all(proxy.proxy_connect_url()) {
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

        if !self.accepts_status(status.as_u16()) {
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

    /// Validate many proxies against every target URL with bounded proxy concurrency.
    ///
    /// `mode` selects strict (all targets) or quorum (any target) admission.
    pub async fn validate_many_against_targets(
        &self,
        proxies: &[Proxy],
        targets: &[ValidationTarget],
        concurrency: usize,
        mode: TargetAdmission,
    ) -> Vec<Proxy> {
        if targets.is_empty() {
            return self.validate_many(proxies, concurrency).await;
        }

        let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
        let mut handles = Vec::with_capacity(proxies.len());

        for proxy in proxies {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let validator = self.clone();
            let targets = targets.to_vec();
            let proxy = proxy.clone();
            handles.push(tokio::spawn(async move {
                let result = validator
                    .validate_one_with_admission(&proxy, &targets, mode)
                    .await;
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

    fn accepts_status(&self, status: u16) -> bool {
        if self.expected_statuses.is_empty() {
            status < 400
        } else {
            self.expected_statuses.contains(&status)
        }
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

fn strict_target_admission_result(original: &Proxy, checks: Vec<Option<Proxy>>) -> Option<Proxy> {
    let mut accepted = None;
    for check in checks {
        accepted = Some(check?);
    }
    accepted.map(|mut proxy| {
        proxy.success_count = original.success_count.saturating_add(1);
        proxy
    })
}

/// Admission policy for validating a proxy against multiple targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetAdmission {
    /// Every target must pass for the proxy to be admitted.
    Strict,
    /// At least one target passing admits the proxy.
    Quorum,
}

/// Quorum admission: alive if any target passed. Uses the first passing check
/// (which carries measured latency/anonymity) as the admitted record.
fn quorum_target_admission_result(original: &Proxy, checks: Vec<Option<Proxy>>) -> Option<Proxy> {
    let mut passed = checks.into_iter().flatten();
    passed.next().map(|mut proxy| {
        proxy.success_count = original.success_count.saturating_add(1);
        proxy
    })
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
    fn validation_target_builders_keep_expected_status_contract() {
        assert_eq!(
            ValidationTarget::new("https://example.com/check"),
            ValidationTarget {
                url: "https://example.com/check".into(),
                expected_statuses: vec![],
            }
        );
        assert_eq!(
            ValidationTarget::with_expected_statuses("https://api.openai.com/v1/models", vec![401]),
            ValidationTarget {
                url: "https://api.openai.com/v1/models".into(),
                expected_statuses: vec![401],
            }
        );
    }

    #[test]
    fn validator_default_status_accepts_only_below_400() {
        let validator = Validator::new("https://example.com/check", 10);

        assert!(validator.accepts_status(200));
        assert!(validator.accepts_status(399));
        assert!(!validator.accepts_status(400));
        assert!(!validator.accepts_status(401));
        assert!(!validator.accepts_status(500));
    }

    #[test]
    fn strict_admission_requires_all_targets() {
        let original = Proxy::new("1.2.3.4", 8080, Protocol::Http);
        let pass = || Some(Proxy::new("1.2.3.4", 8080, Protocol::Http));

        // Two passes → admitted.
        assert!(strict_target_admission_result(&original, vec![pass(), pass()]).is_some());
        // One miss → rejected.
        assert!(strict_target_admission_result(&original, vec![pass(), None]).is_none());
    }

    #[test]
    fn quorum_admission_accepts_any_passing_target() {
        let original = Proxy::new("1.2.3.4", 8080, Protocol::Http);
        let pass = || Some(Proxy::new("1.2.3.4", 8080, Protocol::Http));

        // One of two passes → admitted under quorum (would be rejected by strict).
        let admitted = quorum_target_admission_result(&original, vec![None, pass()]);
        assert!(admitted.is_some());
        assert_eq!(admitted.unwrap().success_count, 1);
        // Zero passes → rejected.
        assert!(quorum_target_admission_result(&original, vec![None, None]).is_none());
    }

    #[test]
    fn validator_expected_statuses_override_default_success_window() {
        let validator = Validator::new("https://api.openai.com/v1/models", 10)
            .with_expected_statuses(vec![401]);

        assert!(!validator.accepts_status(200));
        assert!(validator.accepts_status(401));
        assert!(!validator.accepts_status(403));
        assert!(!validator.accepts_status(500));
    }

    #[test]
    fn matrix_targets_default_when_not_supplied() {
        let targets = matrix_targets(None).unwrap();
        assert_eq!(
            targets,
            DEFAULT_MATRIX_TARGETS
                .iter()
                .map(|target| ValidationTarget::new((*target).to_string()))
                .collect::<Vec<_>>()
        );

        let empty: Vec<ProxyCheckMatrixTarget> = vec![];
        let targets = matrix_targets(Some(&empty)).unwrap();
        assert_eq!(
            targets,
            DEFAULT_MATRIX_TARGETS
                .iter()
                .map(|target| ValidationTarget::new((*target).to_string()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn matrix_targets_accept_legacy_and_structured_entries() {
        let targets = vec![
            ProxyCheckMatrixTarget::Url(" https://example.com/path ".into()),
            ProxyCheckMatrixTarget::Structured {
                url: "https://api.openai.com/v1/models".into(),
                expected_statuses: vec![401],
            },
        ];

        let normalized = matrix_targets(Some(&targets)).unwrap();

        assert_eq!(
            normalized,
            vec![
                ValidationTarget::new("https://example.com/path"),
                ValidationTarget::with_expected_statuses(
                    "https://api.openai.com/v1/models",
                    vec![401]
                ),
            ]
        );
    }

    #[test]
    fn proxy_check_matrix_request_deserializes_target_shapes() {
        let json = r#"{
            "host": "1.2.3.4",
            "port": 8080,
            "protocol": "http",
            "targets": [
                "https://www.cloudflare.com/cdn-cgi/trace",
                {
                    "url": "https://api.openai.com/v1/models",
                    "expected_statuses": [401]
                }
            ]
        }"#;

        let request: ProxyCheckMatrixRequest = serde_json::from_str(json).unwrap();
        let targets = request.targets.unwrap();

        assert_eq!(
            targets,
            vec![
                ProxyCheckMatrixTarget::Url("https://www.cloudflare.com/cdn-cgi/trace".into()),
                ProxyCheckMatrixTarget::Structured {
                    url: "https://api.openai.com/v1/models".into(),
                    expected_statuses: vec![401],
                },
            ]
        );
    }

    #[test]
    fn matrix_targets_reject_invalid_entries() {
        let targets = vec![ProxyCheckMatrixTarget::Url("".to_string())];
        let error = matrix_targets(Some(&targets)).unwrap_err();
        assert_eq!(error.to_string(), "target URL must not be empty");

        let targets = vec![ProxyCheckMatrixTarget::Structured {
            url: "ftp://example.com/file".to_string(),
            expected_statuses: vec![200],
        }];
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
    fn strict_target_admission_accepts_only_when_all_targets_pass() {
        let mut original = Proxy::new("1.2.3.4", 8080, Protocol::Http);
        original.success_count = 7;
        let mut first = original.clone();
        first.latency_ms = Some(50.0);
        let mut second = original.clone();
        second.latency_ms = Some(75.0);

        let accepted =
            strict_target_admission_result(&original, vec![Some(first), Some(second)]).unwrap();

        assert_eq!(accepted.latency_ms, Some(75.0));
        assert_eq!(accepted.success_count, 8);
    }

    #[test]
    fn strict_target_admission_rejects_when_any_target_fails() {
        let original = Proxy::new("1.2.3.4", 8080, Protocol::Http);
        let accepted =
            strict_target_admission_result(&original, vec![Some(original.clone()), None]);

        assert!(accepted.is_none());
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
