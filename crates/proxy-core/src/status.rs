use crate::models::{Protocol, Proxy};
use crate::store::{ProxyStore, RetentionDecision, ScoreExplanation};
use crate::warp::balancer::WarpBalancer;
use crate::xray_status::XrayStatusSnapshot;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write;

const QUALITY_STALE_AFTER_SECS: i64 = 3600;
const MAX_FAILURE_REASON_METRICS: usize = 5;

/// Process and dependency status summary shared by API and MCP surfaces.
#[derive(Debug, Clone, Serialize)]
pub struct ServiceStatus {
    pub version: &'static str,
    pub git_hash: &'static str,
    pub uptime_sec: u64,
    pub release: ReleaseMetadata,
    pub pool: PoolStatus,
    pub quality: QualityStatus,
    pub redis: DependencyStatus,
    pub warp: WarpStatus,
    pub xray: XrayStatus,
}

/// Release metadata exposed through public status surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleaseMetadata {
    pub app_version: &'static str,
    pub git_hash: &'static str,
    pub update_enabled: bool,
    pub update_container: String,
    pub configured_image: String,
    pub image_repo: String,
    pub image_tag: String,
    pub watchtower_url: String,
}

impl ReleaseMetadata {
    /// Build release metadata from the current process environment.
    pub fn from_env(app_version: &'static str, git_hash: &'static str) -> Self {
        Self::from_lookup(app_version, git_hash, |key| std::env::var(key).ok())
    }

    /// Build release metadata from a custom lookup source for tests.
    pub fn from_lookup(
        app_version: &'static str,
        git_hash: &'static str,
        mut get: impl FnMut(&str) -> Option<String>,
    ) -> Self {
        let configured_image = non_empty_env(&mut get, "PROXY_POOL_UPDATE_IMAGE")
            .unwrap_or_else(|| "ghcr.io/iamdreaming/proxy-pool-rust:latest".into());
        let (image_repo, image_tag) = split_image_ref(&configured_image);

        Self {
            app_version,
            git_hash,
            update_enabled: parse_bool_env(get("PROXY_POOL_UPDATE_ENABLED").as_deref()),
            update_container: non_empty_env(&mut get, "PROXY_POOL_UPDATE_CONTAINER")
                .unwrap_or_else(|| "proxy-pool".into()),
            configured_image,
            image_repo,
            image_tag,
            watchtower_url: non_empty_env(&mut get, "PROXY_POOL_UPDATE_WATCHTOWER_URL")
                .unwrap_or_else(|| "http://watchtower-proxy-pool:8080/v1/update".into()),
        }
    }
}

fn non_empty_env(get: &mut impl FnMut(&str) -> Option<String>, key: &str) -> Option<String> {
    get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Parse truthy environment variable values used by release/update wiring.
pub fn parse_bool_env(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()),
        Some(v) if matches!(v.as_str(), "1" | "true" | "yes" | "on")
    )
}

/// Split an image reference into repository and tag, preserving registry ports.
pub fn split_image_ref(image: &str) -> (String, String) {
    let last_colon = image.rfind(':');
    let last_slash = image.rfind('/');
    if let Some(colon_idx) = last_colon
        && last_slash.is_none_or(|slash_idx| colon_idx > slash_idx)
    {
        return (
            image[..colon_idx].to_string(),
            image[colon_idx + 1..].to_string(),
        );
    }
    (image.to_string(), "latest".into())
}

/// Operator-facing pool tier derived from xray and WARP health.
///
/// The tier reflects whether the pool can provide reliable overseas exit
/// capacity.  It is a read-only signal computed from existing status fields
/// and does not change routing or scoring behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PoolTier {
    /// xray active ≥ 3 **and** WARP healthy ≥ 1 — reliable overseas exit.
    Stable,
    /// WARP healthy ≥ 1 but xray active < 3 — degraded overseas capacity.
    Degraded,
    /// WARP healthy ≥ 1, xray not enabled — minimal overseas via WARP only.
    Minimal,
    /// No reliable overseas exit (WARP 0 healthy, xray 0 active).
    #[default]
    Unstable,
}

impl PoolTier {
    /// Derive the pool tier from xray and WARP health signals.
    pub fn from_status(xray_enabled: bool, xray_active: usize, warp_healthy: usize) -> Self {
        if xray_enabled && xray_active >= 3 && warp_healthy >= 1 {
            Self::Stable
        } else if warp_healthy >= 1 && (!xray_enabled || xray_active < 3) {
            if xray_enabled {
                Self::Degraded
            } else {
                Self::Minimal
            }
        } else {
            Self::Unstable
        }
    }
}

/// Proxy pool counts by protocol.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PoolStatus {
    pub http: usize,
    pub https: usize,
    pub socks5: usize,
    pub total: usize,
    /// Derived pool tier reflecting overseas exit reliability.
    pub tier: PoolTier,
}

/// Aggregate proxy quality summary for status and metrics surfaces.
#[derive(Debug, Clone, Serialize)]
pub struct QualityStatus {
    pub total: usize,
    pub score_buckets: QualityScoreBuckets,
    pub recent_samples: usize,
    pub recent_success_rate: Option<f64>,
    pub recent_failures: usize,
    pub stale_proxies: usize,
    pub stale_after_secs: i64,
    pub retention: QualityRetentionStatus,
    pub top_failure_reasons: Vec<FailureReasonCount>,
}

impl Default for QualityStatus {
    fn default() -> Self {
        Self {
            total: 0,
            score_buckets: QualityScoreBuckets::default(),
            recent_samples: 0,
            recent_success_rate: None,
            recent_failures: 0,
            stale_proxies: 0,
            stale_after_secs: QUALITY_STALE_AFTER_SECS,
            retention: QualityRetentionStatus::default(),
            top_failure_reasons: Vec::new(),
        }
    }
}

/// Count of proxies grouped by bounded score buckets.
#[derive(Debug, Clone, Default, Serialize)]
pub struct QualityScoreBuckets {
    pub untested: usize,
    pub poor: usize,
    pub fair: usize,
    pub good: usize,
    pub excellent: usize,
}

/// Count of proxies that currently match retention-risk decisions.
#[derive(Debug, Clone, Default, Serialize)]
pub struct QualityRetentionStatus {
    pub below_min_score: usize,
    pub hard_failure_evict: usize,
}

/// Normalized recent failure reason and count.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FailureReasonCount {
    pub reason: &'static str,
    pub count: usize,
}

/// Dependency health state.
#[derive(Debug, Clone, Serialize)]
pub struct DependencyStatus {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl DependencyStatus {
    pub fn ok() -> Self {
        Self {
            status: "ok",
            message: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            status: "error",
            message: Some(message.into()),
        }
    }

    pub fn is_ok(&self) -> bool {
        self.status == "ok"
    }
}

/// WARP instance summary.
#[derive(Debug, Clone, Default, Serialize)]
pub struct WarpStatus {
    pub configured: usize,
    pub healthy: usize,
}

/// xray integration summary.
#[derive(Debug, Clone, Default, Serialize)]
pub struct XrayStatus {
    pub enabled: bool,
    pub active_nodes: usize,
    pub failed_nodes: usize,
    pub removed_nodes: usize,
    pub total_nodes: usize,
}

impl XrayStatus {
    /// Build a compact service-status summary from the operator snapshot.
    pub fn from_snapshot(snapshot: &XrayStatusSnapshot) -> Self {
        Self {
            enabled: snapshot.enabled,
            active_nodes: snapshot.active_nodes,
            failed_nodes: snapshot.failed_nodes,
            removed_nodes: snapshot.removed_nodes,
            total_nodes: snapshot.total_nodes,
        }
    }
}

/// Collect a service status snapshot without external network calls.
pub async fn collect_service_status(
    store: &ProxyStore,
    balancer: Option<&WarpBalancer>,
    version: &'static str,
    git_hash: &'static str,
    uptime_sec: u64,
    xray: XrayStatus,
) -> ServiceStatus {
    let mut redis_errors = Vec::new();
    let pool = match collect_pool_status(store).await {
        Ok(pool) => pool,
        Err(e) => {
            redis_errors.push(e);
            PoolStatus::default()
        }
    };
    let quality = match collect_quality_status(store, Utc::now()).await {
        Ok(quality) => quality,
        Err(e) => {
            redis_errors.push(e);
            QualityStatus::default()
        }
    };
    let redis = if redis_errors.is_empty() {
        DependencyStatus::ok()
    } else {
        DependencyStatus::error(redis_errors.join("; "))
    };
    let warp_status = collect_warp_status(balancer).await;

    ServiceStatus {
        version,
        git_hash,
        uptime_sec,
        release: ReleaseMetadata::from_env(version, git_hash),
        pool: PoolStatus {
            tier: PoolTier::from_status(xray.enabled, xray.active_nodes, warp_status.healthy),
            ..pool
        },
        quality,
        redis,
        warp: warp_status,
        xray,
    }
}

/// Check required dependencies for readiness.
pub async fn collect_readiness(store: &ProxyStore) -> DependencyStatus {
    match collect_pool_status(store).await {
        Ok(_) => DependencyStatus::ok(),
        Err(e) => DependencyStatus::error(e),
    }
}

async fn collect_pool_status(store: &ProxyStore) -> Result<PoolStatus, String> {
    let http = count_protocol(store, Protocol::Http).await?;
    let https = count_protocol(store, Protocol::Https).await?;
    let socks5 = count_protocol(store, Protocol::Socks5).await?;
    Ok(PoolStatus {
        http,
        https,
        socks5,
        total: http + https + socks5,
        tier: PoolTier::default(), // overridden in collect_service_status
    })
}

async fn collect_quality_status(
    store: &ProxyStore,
    now: DateTime<Utc>,
) -> Result<QualityStatus, String> {
    let mut proxies = Vec::new();
    for protocol in Protocol::all() {
        proxies.extend(
            store
                .all(*protocol)
                .await
                .map_err(|e| format!("redis quality scan failed: {e}"))?,
        );
    }
    Ok(build_quality_status(
        &proxies,
        |proxy| store.explain(proxy),
        now,
    ))
}

fn build_quality_status(
    proxies: &[Proxy],
    explain: impl Fn(&Proxy) -> ScoreExplanation,
    now: DateTime<Utc>,
) -> QualityStatus {
    let mut status = QualityStatus {
        stale_after_secs: QUALITY_STALE_AFTER_SECS,
        ..Default::default()
    };
    let mut recent_successes = 0usize;
    let mut failure_reasons: BTreeMap<&'static str, usize> = BTreeMap::new();

    for proxy in proxies {
        let explanation = explain(proxy);
        let trend = &explanation.trend;
        status.total += 1;
        status.recent_samples += trend.recent_samples;
        status.recent_failures += trend.recent_failures;
        recent_successes += trend.recent_samples.saturating_sub(trend.recent_failures);

        let last_checked = last_checked_unix_secs(proxy, &explanation);
        if is_stale(last_checked, now) {
            status.stale_proxies += 1;
        }
        add_score_bucket(&mut status.score_buckets, last_checked, explanation.score);
        add_retention_count(&mut status.retention, explanation.retention);
        add_failure_reasons(&mut failure_reasons, proxy);
    }

    status.recent_success_rate = (status.recent_samples > 0)
        .then_some(recent_successes as f64 / status.recent_samples as f64);
    status.top_failure_reasons = top_failure_reasons(failure_reasons);
    status
}

fn last_checked_unix_secs(proxy: &Proxy, explanation: &ScoreExplanation) -> Option<i64> {
    explanation
        .trend
        .last_checked_at_unix_secs
        .or_else(|| proxy.last_check.map(|checked| checked.timestamp()))
}

fn is_stale(last_checked: Option<i64>, now: DateTime<Utc>) -> bool {
    last_checked
        .is_none_or(|checked| now.timestamp().saturating_sub(checked) >= QUALITY_STALE_AFTER_SECS)
}

fn add_score_bucket(buckets: &mut QualityScoreBuckets, last_checked: Option<i64>, score: f64) {
    if last_checked.is_none() {
        buckets.untested += 1;
    } else if score >= 0.8 {
        buckets.excellent += 1;
    } else if score >= 0.6 {
        buckets.good += 1;
    } else if score >= 0.3 {
        buckets.fair += 1;
    } else {
        buckets.poor += 1;
    }
}

fn add_retention_count(retention: &mut QualityRetentionStatus, decision: RetentionDecision) {
    match decision {
        RetentionDecision::Keep => {}
        RetentionDecision::BelowMinScore => retention.below_min_score += 1,
        RetentionDecision::HardFailureEvict => retention.hard_failure_evict += 1,
    }
}

fn add_failure_reasons(reasons: &mut BTreeMap<&'static str, usize>, proxy: &Proxy) {
    for sample in proxy
        .quality_history
        .samples
        .iter()
        .filter(|sample| !sample.success)
    {
        let reason = normalize_failure_reason(sample.error.as_deref());
        *reasons.entry(reason).or_default() += 1;
    }
}

fn normalize_failure_reason(error: Option<&str>) -> &'static str {
    let Some(error) = error.map(str::trim).filter(|value| !value.is_empty()) else {
        return "unknown";
    };
    let lower = error.to_ascii_lowercase();
    if lower.contains("timeout") {
        "timeout"
    } else if lower.contains("bad_status") || lower.contains("bad status") {
        "bad_status"
    } else if lower.contains("body_read_failed") || lower.contains("body read failed") {
        "body_read_failed"
    } else if lower.contains("invalid_proxy_url") || lower.contains("invalid proxy url") {
        "invalid_proxy_url"
    } else if lower.contains("client_build_failed") || lower.contains("client build failed") {
        "client_build_failed"
    } else if lower.contains("request_failed") || lower.contains("request failed") {
        "request_failed"
    } else if lower.contains("circuit") {
        "circuit_open"
    } else if lower.contains("validation_failed") || lower.contains("validation failed") {
        "validation_failed"
    } else {
        "other"
    }
}

fn top_failure_reasons(reasons: BTreeMap<&'static str, usize>) -> Vec<FailureReasonCount> {
    let mut counts: Vec<FailureReasonCount> = reasons
        .into_iter()
        .map(|(reason, count)| FailureReasonCount { reason, count })
        .collect();
    counts.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.reason.cmp(right.reason))
    });
    counts.truncate(MAX_FAILURE_REASON_METRICS);
    counts
}

async fn count_protocol(store: &ProxyStore, protocol: Protocol) -> Result<usize, String> {
    store
        .count(protocol)
        .await
        .map_err(|e| format!("redis count failed: {e}"))
}

async fn collect_warp_status(balancer: Option<&WarpBalancer>) -> WarpStatus {
    let Some(balancer) = balancer else {
        return WarpStatus::default();
    };
    let instances = balancer.all_list().await;
    let configured = instances.len();
    let healthy = instances.iter().filter(|i| i.healthy).count();
    WarpStatus {
        configured,
        healthy,
    }
}

/// Render Prometheus text metrics for a status snapshot.
pub fn render_prometheus_metrics(status: &ServiceStatus) -> String {
    let mut out = String::new();
    let redis_ready = usize::from(status.redis.is_ok());

    writeln!(out, "# HELP proxy_pool_size Number of proxies in pool").ok();
    writeln!(out, "# TYPE proxy_pool_size gauge").ok();
    writeln!(
        out,
        "proxy_pool_size{{protocol=\"http\"}} {}",
        status.pool.http
    )
    .ok();
    writeln!(
        out,
        "proxy_pool_size{{protocol=\"https\"}} {}",
        status.pool.https
    )
    .ok();
    writeln!(
        out,
        "proxy_pool_size{{protocol=\"socks5\"}} {}",
        status.pool.socks5
    )
    .ok();
    writeln!(
        out,
        "proxy_pool_size{{protocol=\"total\"}} {}",
        status.pool.total
    )
    .ok();

    writeln!(
        out,
        "# HELP proxy_pool_tier Pool tier reflecting overseas exit reliability (0=unstable, 1=minimal, 2=degraded, 3=stable)"
    )
    .ok();
    writeln!(out, "# TYPE proxy_pool_tier gauge").ok();
    let tier_value = match status.pool.tier {
        PoolTier::Unstable => 0,
        PoolTier::Minimal => 1,
        PoolTier::Degraded => 2,
        PoolTier::Stable => 3,
    };
    writeln!(out, "proxy_pool_tier {tier_value}").ok();

    writeln!(
        out,
        "# HELP proxy_quality_score_bucket Number of proxies by quality score bucket"
    )
    .ok();
    writeln!(out, "# TYPE proxy_quality_score_bucket gauge").ok();
    writeln!(
        out,
        "proxy_quality_score_bucket{{bucket=\"untested\"}} {}",
        status.quality.score_buckets.untested
    )
    .ok();
    writeln!(
        out,
        "proxy_quality_score_bucket{{bucket=\"poor\"}} {}",
        status.quality.score_buckets.poor
    )
    .ok();
    writeln!(
        out,
        "proxy_quality_score_bucket{{bucket=\"fair\"}} {}",
        status.quality.score_buckets.fair
    )
    .ok();
    writeln!(
        out,
        "proxy_quality_score_bucket{{bucket=\"good\"}} {}",
        status.quality.score_buckets.good
    )
    .ok();
    writeln!(
        out,
        "proxy_quality_score_bucket{{bucket=\"excellent\"}} {}",
        status.quality.score_buckets.excellent
    )
    .ok();

    writeln!(
        out,
        "# HELP proxy_quality_recent_samples_total Recent validation samples retained in the pool"
    )
    .ok();
    writeln!(out, "# TYPE proxy_quality_recent_samples_total gauge").ok();
    writeln!(
        out,
        "proxy_quality_recent_samples_total {}",
        status.quality.recent_samples
    )
    .ok();
    writeln!(
        out,
        "# HELP proxy_quality_recent_success_rate Aggregate recent validation success rate"
    )
    .ok();
    writeln!(out, "# TYPE proxy_quality_recent_success_rate gauge").ok();
    writeln!(
        out,
        "proxy_quality_recent_success_rate {}",
        status.quality.recent_success_rate.unwrap_or(0.0)
    )
    .ok();
    writeln!(
        out,
        "# HELP proxy_quality_recent_failures_total Recent validation failures retained in the pool"
    )
    .ok();
    writeln!(out, "# TYPE proxy_quality_recent_failures_total gauge").ok();
    writeln!(
        out,
        "proxy_quality_recent_failures_total {}",
        status.quality.recent_failures
    )
    .ok();
    writeln!(
        out,
        "# HELP proxy_quality_stale_proxies_total Proxies with no recent quality check"
    )
    .ok();
    writeln!(out, "# TYPE proxy_quality_stale_proxies_total gauge").ok();
    writeln!(
        out,
        "proxy_quality_stale_proxies_total {}",
        status.quality.stale_proxies
    )
    .ok();
    writeln!(
        out,
        "# HELP proxy_quality_stale_after_seconds Age threshold used for stale quality classification"
    )
    .ok();
    writeln!(out, "# TYPE proxy_quality_stale_after_seconds gauge").ok();
    writeln!(
        out,
        "proxy_quality_stale_after_seconds {}",
        status.quality.stale_after_secs
    )
    .ok();
    writeln!(
        out,
        "# HELP proxy_quality_retention_candidates Number of proxies matching retention-risk decisions"
    )
    .ok();
    writeln!(out, "# TYPE proxy_quality_retention_candidates gauge").ok();
    writeln!(
        out,
        "proxy_quality_retention_candidates{{decision=\"below_min_score\"}} {}",
        status.quality.retention.below_min_score
    )
    .ok();
    writeln!(
        out,
        "proxy_quality_retention_candidates{{decision=\"hard_failure_evict\"}} {}",
        status.quality.retention.hard_failure_evict
    )
    .ok();
    writeln!(
        out,
        "# HELP proxy_quality_failure_reasons_total Recent validation failures by normalized reason"
    )
    .ok();
    writeln!(out, "# TYPE proxy_quality_failure_reasons_total gauge").ok();
    for reason in &status.quality.top_failure_reasons {
        writeln!(
            out,
            "proxy_quality_failure_reasons_total{{reason=\"{}\"}} {}",
            reason.reason, reason.count
        )
        .ok();
    }

    writeln!(out, "# HELP proxy_redis_ready Redis readiness state").ok();
    writeln!(out, "# TYPE proxy_redis_ready gauge").ok();
    writeln!(out, "proxy_redis_ready {redis_ready}").ok();

    writeln!(
        out,
        "# HELP proxy_warp_instances_configured Configured WARP instances"
    )
    .ok();
    writeln!(out, "# TYPE proxy_warp_instances_configured gauge").ok();
    writeln!(
        out,
        "proxy_warp_instances_configured {}",
        status.warp.configured
    )
    .ok();
    writeln!(
        out,
        "# HELP proxy_warp_instances_healthy Healthy WARP instances"
    )
    .ok();
    writeln!(out, "# TYPE proxy_warp_instances_healthy gauge").ok();
    writeln!(out, "proxy_warp_instances_healthy {}", status.warp.healthy).ok();

    writeln!(out, "# HELP proxy_xray_active_nodes Active xray nodes").ok();
    writeln!(out, "# TYPE proxy_xray_active_nodes gauge").ok();
    writeln!(out, "proxy_xray_active_nodes {}", status.xray.active_nodes).ok();
    writeln!(out, "# HELP proxy_xray_failed_nodes Failed xray nodes").ok();
    writeln!(out, "# TYPE proxy_xray_failed_nodes gauge").ok();
    writeln!(out, "proxy_xray_failed_nodes {}", status.xray.failed_nodes).ok();

    writeln!(out, "# HELP proxy_uptime_seconds Process uptime in seconds").ok();
    writeln!(out, "# TYPE proxy_uptime_seconds gauge").ok();
    writeln!(out, "proxy_uptime_seconds {}", status.uptime_sec).ok();

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ScoreWeights;
    use crate::store::explain_score;
    use chrono::TimeZone;

    /// Closed set of normalized failure-reason labels emitted by metrics.
    const FAILURE_REASONS: &[&str] = &[
        "unknown",
        "timeout",
        "bad_status",
        "body_read_failed",
        "invalid_proxy_url",
        "client_build_failed",
        "request_failed",
        "circuit_open",
        "validation_failed",
        "other",
    ];

    fn default_weights() -> ScoreWeights {
        ScoreWeights {
            latency: 0.5,
            success: 0.3,
            anonymity: 0.2,
        }
    }

    fn explain_for_test(proxy: &Proxy) -> ScoreExplanation {
        explain_score(proxy, &default_weights(), 0.1)
    }

    #[test]
    fn service_status_serializes_required_sections() {
        let status = ServiceStatus {
            version: "0.1.0",
            git_hash: "abc1234",
            uptime_sec: 42,
            release: ReleaseMetadata::from_lookup("0.1.0", "abc1234", |_| None),
            pool: PoolStatus {
                http: 2,
                https: 1,
                socks5: 3,
                total: 6,
                tier: PoolTier::Stable,
            },
            quality: QualityStatus::default(),
            redis: DependencyStatus::ok(),
            warp: WarpStatus {
                configured: 3,
                healthy: 2,
            },
            xray: XrayStatus {
                enabled: true,
                active_nodes: 5,
                failed_nodes: 1,
                removed_nodes: 2,
                total_nodes: 8,
            },
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"version\":\"0.1.0\""));
        assert!(json.contains("\"git_hash\":\"abc1234\""));
        assert!(json.contains("\"release\""));
        assert!(json.contains("\"configured_image\""));
        assert!(json.contains("\"uptime_sec\":42"));
        assert!(json.contains("\"total\":6"));
        assert!(json.contains("\"quality\""));
        assert!(json.contains("\"redis\""));
        assert!(json.contains("\"warp\""));
        assert!(json.contains("\"xray\""));
    }

    #[test]
    fn dependency_status_error_includes_message() {
        let status = DependencyStatus::error("redis down");
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"status\":\"error\""));
        assert!(json.contains("redis down"));
    }

    #[test]
    fn release_metadata_uses_defaults_without_runtime_env() {
        let metadata = ReleaseMetadata::from_lookup("0.1.0", "abc1234", |_| None);
        assert_eq!(metadata.app_version, "0.1.0");
        assert_eq!(metadata.git_hash, "abc1234");
        assert!(!metadata.update_enabled);
        assert_eq!(metadata.update_container, "proxy-pool");
        assert_eq!(
            metadata.configured_image,
            "ghcr.io/iamdreaming/proxy-pool-rust:latest"
        );
        assert_eq!(metadata.image_repo, "ghcr.io/iamdreaming/proxy-pool-rust");
        assert_eq!(metadata.image_tag, "latest");
    }

    #[test]
    fn release_metadata_reads_update_env() {
        let metadata = ReleaseMetadata::from_lookup("0.1.0", "abc1234", |key| match key {
            "PROXY_POOL_UPDATE_ENABLED" => Some("yes".into()),
            "PROXY_POOL_UPDATE_CONTAINER" => Some("proxy-pool-blue".into()),
            "PROXY_POOL_UPDATE_IMAGE" => Some("localhost:5000/proxy-pool:test".into()),
            "PROXY_POOL_UPDATE_WATCHTOWER_URL" => Some("http://watchtower/v1/update".into()),
            _ => None,
        });

        assert!(metadata.update_enabled);
        assert_eq!(metadata.update_container, "proxy-pool-blue");
        assert_eq!(metadata.configured_image, "localhost:5000/proxy-pool:test");
        assert_eq!(metadata.image_repo, "localhost:5000/proxy-pool");
        assert_eq!(metadata.image_tag, "test");
        assert_eq!(metadata.watchtower_url, "http://watchtower/v1/update");
    }

    #[test]
    fn parse_bool_env_accepts_truthy_values() {
        for value in ["1", "true", "TRUE", "yes", "on"] {
            assert!(parse_bool_env(Some(value)));
        }
        for value in [None, Some(""), Some("false"), Some("0"), Some("off")] {
            assert!(!parse_bool_env(value));
        }
    }

    #[test]
    fn split_image_ref_handles_registry_ports_and_missing_tags() {
        assert_eq!(
            split_image_ref("localhost:5000/proxy-pool:test"),
            ("localhost:5000/proxy-pool".into(), "test".into())
        );
        assert_eq!(
            split_image_ref("ghcr.io/iamdreaming/proxy-pool-rust"),
            (
                "ghcr.io/iamdreaming/proxy-pool-rust".into(),
                "latest".into()
            )
        );
    }

    #[test]
    fn quality_status_empty_pool_is_deterministic() {
        let now = Utc.with_ymd_and_hms(2026, 7, 7, 0, 0, 0).unwrap();
        let quality = build_quality_status(&[], explain_for_test, now);

        assert_eq!(quality.total, 0);
        assert_eq!(quality.recent_samples, 0);
        assert_eq!(quality.recent_success_rate, None);
        assert_eq!(quality.recent_failures, 0);
        assert_eq!(quality.stale_proxies, 0);
        assert_eq!(quality.stale_after_secs, QUALITY_STALE_AFTER_SECS);
        assert_eq!(quality.score_buckets.untested, 0);
        assert!(quality.top_failure_reasons.is_empty());
    }

    #[test]
    fn quality_status_aggregates_buckets_stale_retention_and_failures() {
        let now = Utc.with_ymd_and_hms(2026, 7, 7, 0, 0, 0).unwrap();
        let old = now - chrono::Duration::seconds(QUALITY_STALE_AFTER_SECS + 60);

        let untested = Proxy::new("1.1.1.1", 80, Protocol::Http);

        let mut poor_stale = Proxy::new("2.2.2.2", 8080, Protocol::Http);
        poor_stale.last_check = Some(old);
        poor_stale.latency_ms = Some(3000.0);
        poor_stale.fail_count = 1;
        poor_stale
            .quality_history
            .record_failure(old, "timeout while checking http://2.2.2.2:8080");

        let mut excellent = Proxy::new("3.3.3.3", 8080, Protocol::Http);
        excellent.last_check = Some(now);
        excellent.latency_ms = Some(100.0);
        excellent.success_count = 10;
        excellent.anonymity = Some(crate::models::Anonymity::Elite);
        excellent.quality_history.record_success(now, Some(100.0));

        let mut hard_failure = Proxy::new("4.4.4.4", 8080, Protocol::Http);
        hard_failure.last_check = Some(now);
        hard_failure.latency_ms = Some(100.0);
        hard_failure.fail_count = 9;
        hard_failure.anonymity = Some(crate::models::Anonymity::Elite);
        hard_failure
            .quality_history
            .record_failure(now, "validation_failed");

        let quality = build_quality_status(
            &[untested, poor_stale, excellent, hard_failure],
            explain_for_test,
            now,
        );

        assert_eq!(quality.total, 4);
        assert_eq!(quality.score_buckets.untested, 1);
        assert_eq!(quality.score_buckets.poor, 1);
        assert_eq!(quality.score_buckets.good, 1);
        assert_eq!(quality.score_buckets.excellent, 1);
        assert_eq!(quality.stale_proxies, 2);
        assert_eq!(quality.recent_samples, 3);
        assert_eq!(quality.recent_failures, 2);
        assert_eq!(quality.recent_success_rate, Some(1.0 / 3.0));
        assert_eq!(quality.retention.below_min_score, 0);
        assert_eq!(quality.retention.hard_failure_evict, 1);
        assert_eq!(
            quality.top_failure_reasons,
            vec![
                FailureReasonCount {
                    reason: "timeout",
                    count: 1,
                },
                FailureReasonCount {
                    reason: "validation_failed",
                    count: 1,
                },
            ]
        );
    }

    #[test]
    fn failure_reason_normalization_is_bounded() {
        let cases: &[(Option<&str>, &str)] = &[
            (None, "unknown"),
            (Some(""), "unknown"),
            (Some("   "), "unknown"),
            (Some("connection timeout after 5s"), "timeout"),
            (Some("bad status: 500"), "bad_status"),
            (Some("bad_status from upstream"), "bad_status"),
            (Some("body read failed: eof"), "body_read_failed"),
            (Some("body_read_failed"), "body_read_failed"),
            (Some("invalid proxy url: not-a-url"), "invalid_proxy_url"),
            (Some("invalid_proxy_url"), "invalid_proxy_url"),
            (Some("client build failed: tls"), "client_build_failed"),
            (Some("client_build_failed"), "client_build_failed"),
            (
                Some("request failed for http://evil.example/path via 1.2.3.4:8080"),
                "request_failed",
            ),
            (Some("request_failed"), "request_failed"),
            (Some("circuit open until probe"), "circuit_open"),
            (Some("validation failed on target"), "validation_failed"),
            (Some("validation_failed"), "validation_failed"),
            (Some("opaque upstream said 1.2.3.4:8080 failed"), "other"),
            (
                Some("upstream returned garbage for https://evil.example/x"),
                "other",
            ),
        ];
        for &(input, expected) in cases {
            assert_eq!(normalize_failure_reason(input), expected, "{input:?}");
            assert!(
                FAILURE_REASONS.contains(&expected),
                "expected {expected} must stay in closed set"
            );
        }
    }

    /// Parse Prometheus sample lines into (metric_name, labels, value_token).
    /// Skips HELP/TYPE comment lines. Labels are key="value" pairs inside `{}`.
    type PrometheusLabels = Vec<(String, String)>;
    type PrometheusSample = (String, PrometheusLabels, String);

    fn parse_prometheus_samples(metrics: &str) -> Vec<PrometheusSample> {
        let mut samples = Vec::new();
        for line in metrics.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (name_and_labels, value) = match line.rsplit_once(' ') {
                Some(parts) => parts,
                None => continue,
            };
            if let Some(brace) = name_and_labels.find('{') {
                let name = &name_and_labels[..brace];
                let labels_body = name_and_labels[brace + 1..].trim_end_matches('}');
                let mut labels = PrometheusLabels::new();
                for part in labels_body.split(',') {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }
                    let Some((key, raw_value)) = part.split_once('=') else {
                        continue;
                    };
                    let value = raw_value.trim().trim_matches('"').to_string();
                    labels.push((key.trim().to_string(), value));
                }
                samples.push((name.to_string(), labels, value.to_string()));
            } else {
                samples.push((name_and_labels.to_string(), Vec::new(), value.to_string()));
            }
        }
        samples
    }

    fn sample_status_for_metrics() -> ServiceStatus {
        ServiceStatus {
            version: "0.1.0",
            git_hash: "abc1234",
            uptime_sec: 42,
            release: ReleaseMetadata::from_lookup("0.1.0", "abc1234", |_| None),
            pool: PoolStatus {
                http: 2,
                https: 1,
                socks5: 3,
                total: 6,
                tier: PoolTier::Stable,
            },
            quality: QualityStatus {
                total: 6,
                score_buckets: QualityScoreBuckets {
                    untested: 1,
                    poor: 2,
                    fair: 1,
                    good: 1,
                    excellent: 1,
                },
                recent_samples: 10,
                recent_success_rate: Some(0.7),
                recent_failures: 3,
                stale_proxies: 2,
                stale_after_secs: QUALITY_STALE_AFTER_SECS,
                retention: QualityRetentionStatus {
                    below_min_score: 2,
                    hard_failure_evict: 1,
                },
                top_failure_reasons: vec![
                    FailureReasonCount {
                        reason: "timeout",
                        count: 2,
                    },
                    FailureReasonCount {
                        reason: "other",
                        count: 1,
                    },
                ],
            },
            redis: DependencyStatus::ok(),
            warp: WarpStatus {
                configured: 3,
                healthy: 2,
            },
            xray: XrayStatus {
                enabled: true,
                active_nodes: 5,
                failed_nodes: 1,
                removed_nodes: 2,
                total_nodes: 8,
            },
        }
    }

    #[test]
    fn metrics_include_pool_dependency_warp_and_xray_values() {
        let status = sample_status_for_metrics();
        let metrics = render_prometheus_metrics(&status);
        assert!(metrics.contains("proxy_pool_size{protocol=\"http\"} 2"));
        assert!(metrics.contains("proxy_pool_size{protocol=\"total\"} 6"));
        assert!(metrics.contains("proxy_quality_score_bucket{bucket=\"poor\"} 2"));
        assert!(metrics.contains("proxy_quality_recent_samples_total 10"));
        assert!(metrics.contains("proxy_quality_recent_success_rate 0.7"));
        assert!(metrics.contains("proxy_quality_recent_failures_total 3"));
        assert!(metrics.contains("proxy_quality_stale_proxies_total 2"));
        assert!(
            metrics.contains("proxy_quality_retention_candidates{decision=\"below_min_score\"} 2")
        );
        assert!(metrics.contains("proxy_quality_failure_reasons_total{reason=\"timeout\"} 2"));
        assert!(metrics.contains("proxy_redis_ready 1"));
        assert!(metrics.contains("proxy_warp_instances_configured 3"));
        assert!(metrics.contains("proxy_xray_active_nodes 5"));
        assert!(metrics.contains("proxy_xray_failed_nodes 1"));
        assert!(metrics.contains("proxy_pool_tier 3"));
    }

    #[test]
    fn metrics_label_allowlist_is_closed() {
        // Allowed labeled metrics and their label value sets (compile-time fixed).
        const POOL_PROTOCOLS: &[&str] = &["http", "https", "socks5", "total"];
        const SCORE_BUCKETS: &[&str] = &["untested", "poor", "fair", "good", "excellent"];
        const RETENTION_DECISIONS: &[&str] = &["below_min_score", "hard_failure_evict"];
        const UNLABELED_METRICS: &[&str] = &[
            "proxy_pool_tier",
            "proxy_quality_recent_samples_total",
            "proxy_quality_recent_success_rate",
            "proxy_quality_recent_failures_total",
            "proxy_quality_stale_proxies_total",
            "proxy_quality_stale_after_seconds",
            "proxy_redis_ready",
            "proxy_warp_instances_configured",
            "proxy_warp_instances_healthy",
            "proxy_xray_active_nodes",
            "proxy_xray_failed_nodes",
            "proxy_uptime_seconds",
        ];

        let labeled: &[(&str, &str, &[&str])] = &[
            ("proxy_pool_size", "protocol", POOL_PROTOCOLS),
            ("proxy_quality_score_bucket", "bucket", SCORE_BUCKETS),
            (
                "proxy_quality_retention_candidates",
                "decision",
                RETENTION_DECISIONS,
            ),
            (
                "proxy_quality_failure_reasons_total",
                "reason",
                FAILURE_REASONS,
            ),
        ];

        let metrics = render_prometheus_metrics(&sample_status_for_metrics());
        let samples = parse_prometheus_samples(&metrics);
        assert!(
            !samples.is_empty(),
            "expected at least one prometheus sample line"
        );

        for (name, labels, _value) in &samples {
            if labels.is_empty() {
                assert!(
                    UNLABELED_METRICS.contains(&name.as_str()),
                    "unexpected unlabeled metric: {name}"
                );
                continue;
            }
            let Some((_, key, allowed)) = labeled
                .iter()
                .find(|(metric, _, _)| *metric == name.as_str())
            else {
                panic!("unexpected labeled metric: {name}");
            };
            assert_eq!(labels.len(), 1, "{name} must have exactly one label");
            assert_eq!(labels[0].0, *key, "{name} label key");
            assert!(
                allowed.contains(&labels[0].1.as_str()),
                "disallowed {key} label on {name}: {}",
                labels[0].1
            );
        }
    }

    #[test]
    fn metrics_failure_reason_render_has_no_high_cardinality_substrings() {
        let now = Utc.with_ymd_and_hms(2026, 7, 7, 0, 0, 0).unwrap();

        let mut request_failed_proxy = Proxy::new("1.1.1.1", 8080, Protocol::Http);
        request_failed_proxy.last_check = Some(now);
        // Distinct timestamps: quality_history dedupes identical consecutive samples.
        // Three identical raw failures → count 3 after normalize.
        for i in 0..3 {
            request_failed_proxy.quality_history.record_failure(
                now + chrono::Duration::seconds(i),
                "request failed for http://evil.example/path via 1.2.3.4:8080",
            );
        }

        let mut other_proxy = Proxy::new("2.2.2.2", 8080, Protocol::Http);
        other_proxy.last_check = Some(now);
        other_proxy
            .quality_history
            .record_failure(now, "opaque upstream said 1.2.3.4:8080 failed");
        other_proxy.quality_history.record_failure(
            now + chrono::Duration::seconds(1),
            "upstream returned garbage for https://evil.example/x",
        );

        let quality =
            build_quality_status(&[request_failed_proxy, other_proxy], explain_for_test, now);
        let mut status = sample_status_for_metrics();
        status.quality = quality;

        let metrics = render_prometheus_metrics(&status);

        assert!(
            !metrics.contains("http://evil.example"),
            "metrics leaked raw URL substring"
        );
        assert!(
            !metrics.contains("https://evil.example"),
            "metrics leaked raw HTTPS URL substring"
        );
        assert!(
            !metrics.contains("1.2.3.4:8080"),
            "metrics leaked raw host:port substring"
        );

        assert!(
            metrics.contains("proxy_quality_failure_reasons_total{reason=\"request_failed\"} 3")
        );
        assert!(metrics.contains("proxy_quality_failure_reasons_total{reason=\"other\"} 2"));

        for (name, labels, _value) in parse_prometheus_samples(&metrics) {
            if name != "proxy_quality_failure_reasons_total" {
                continue;
            }
            assert_eq!(labels.len(), 1);
            assert_eq!(labels[0].0, "reason");
            assert!(
                FAILURE_REASONS.contains(&labels[0].1.as_str()),
                "reason label not in closed set: {}",
                labels[0].1
            );
            assert!(
                !labels[0].1.contains("http://") && !labels[0].1.contains("https://"),
                "reason label looks like a URL: {}",
                labels[0].1
            );
        }
    }

    #[test]
    fn pool_tier_from_status_covers_all_combinations() {
        // Stable: xray enabled + active ≥ 3 + WARP healthy ≥ 1
        assert_eq!(PoolTier::from_status(true, 3, 1), PoolTier::Stable);
        assert_eq!(PoolTier::from_status(true, 10, 5), PoolTier::Stable);

        // Degraded: WARP healthy ≥ 1, xray enabled but active < 3
        assert_eq!(PoolTier::from_status(true, 2, 1), PoolTier::Degraded);
        assert_eq!(PoolTier::from_status(true, 0, 3), PoolTier::Degraded);

        // Minimal: WARP healthy ≥ 1, xray not enabled
        assert_eq!(PoolTier::from_status(false, 0, 1), PoolTier::Minimal);
        assert_eq!(PoolTier::from_status(false, 0, 5), PoolTier::Minimal);

        // Unstable: WARP 0 healthy
        assert_eq!(PoolTier::from_status(true, 5, 0), PoolTier::Unstable);
        assert_eq!(PoolTier::from_status(false, 0, 0), PoolTier::Unstable);
    }

    #[test]
    fn pool_tier_default_is_unstable() {
        assert_eq!(PoolTier::default(), PoolTier::Unstable);
    }

    #[test]
    fn pool_tier_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&PoolTier::Stable).unwrap(),
            "\"stable\""
        );
        assert_eq!(
            serde_json::to_string(&PoolTier::Degraded).unwrap(),
            "\"degraded\""
        );
        assert_eq!(
            serde_json::to_string(&PoolTier::Minimal).unwrap(),
            "\"minimal\""
        );
        assert_eq!(
            serde_json::to_string(&PoolTier::Unstable).unwrap(),
            "\"unstable\""
        );
    }
}
