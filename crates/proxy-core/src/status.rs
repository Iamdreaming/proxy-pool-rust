use crate::models::Protocol;
use crate::store::ProxyStore;
use crate::warp::balancer::WarpBalancer;
use crate::xray_status::XrayStatusSnapshot;
use serde::Serialize;
use std::fmt::Write;

/// Process and dependency status summary shared by API and MCP surfaces.
#[derive(Debug, Clone, Serialize)]
pub struct ServiceStatus {
    pub version: &'static str,
    pub git_hash: &'static str,
    pub uptime_sec: u64,
    pub release: ReleaseMetadata,
    pub pool: PoolStatus,
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

/// Proxy pool counts by protocol.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PoolStatus {
    pub http: usize,
    pub https: usize,
    pub socks5: usize,
    pub total: usize,
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
    let (pool, redis) = match collect_pool_status(store).await {
        Ok(pool) => (pool, DependencyStatus::ok()),
        Err(e) => (PoolStatus::default(), DependencyStatus::error(e)),
    };

    ServiceStatus {
        version,
        git_hash,
        uptime_sec,
        release: ReleaseMetadata::from_env(version, git_hash),
        pool,
        redis,
        warp: collect_warp_status(balancer).await,
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
    })
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
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"version\":\"0.1.0\""));
        assert!(json.contains("\"git_hash\":\"abc1234\""));
        assert!(json.contains("\"release\""));
        assert!(json.contains("\"configured_image\""));
        assert!(json.contains("\"uptime_sec\":42"));
        assert!(json.contains("\"total\":6"));
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
    fn metrics_include_pool_dependency_warp_and_xray_values() {
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
        };

        let metrics = render_prometheus_metrics(&status);
        assert!(metrics.contains("proxy_pool_size{protocol=\"http\"} 2"));
        assert!(metrics.contains("proxy_pool_size{protocol=\"total\"} 6"));
        assert!(metrics.contains("proxy_redis_ready 1"));
        assert!(metrics.contains("proxy_warp_instances_configured 3"));
        assert!(metrics.contains("proxy_xray_active_nodes 5"));
        assert!(metrics.contains("proxy_xray_failed_nodes 1"));
    }
}
