use crate::models::Protocol;
use crate::store::ProxyStore;
use crate::warp::balancer::WarpBalancer;
use serde::Serialize;
use std::fmt::Write;

/// Process and dependency status summary shared by API and MCP surfaces.
#[derive(Debug, Clone, Serialize)]
pub struct ServiceStatus {
    pub version: &'static str,
    pub git_hash: &'static str,
    pub uptime_sec: u64,
    pub pool: PoolStatus,
    pub redis: DependencyStatus,
    pub warp: WarpStatus,
    pub xray: XrayStatus,
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
    pub active_nodes: usize,
}

/// Collect a service status snapshot without external network calls.
pub async fn collect_service_status(
    store: &ProxyStore,
    balancer: Option<&WarpBalancer>,
    version: &'static str,
    git_hash: &'static str,
    uptime_sec: u64,
    xray_active_nodes: usize,
) -> ServiceStatus {
    let (pool, redis) = match collect_pool_status(store).await {
        Ok(pool) => (pool, DependencyStatus::ok()),
        Err(e) => (PoolStatus::default(), DependencyStatus::error(e)),
    };

    ServiceStatus {
        version,
        git_hash,
        uptime_sec,
        pool,
        redis,
        warp: collect_warp_status(balancer).await,
        xray: XrayStatus {
            active_nodes: xray_active_nodes,
        },
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
            xray: XrayStatus { active_nodes: 5 },
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"version\":\"0.1.0\""));
        assert!(json.contains("\"git_hash\":\"abc1234\""));
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
    fn metrics_include_pool_dependency_warp_and_xray_values() {
        let status = ServiceStatus {
            version: "0.1.0",
            git_hash: "abc1234",
            uptime_sec: 42,
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
            xray: XrayStatus { active_nodes: 5 },
        };

        let metrics = render_prometheus_metrics(&status);
        assert!(metrics.contains("proxy_pool_size{protocol=\"http\"} 2"));
        assert!(metrics.contains("proxy_pool_size{protocol=\"total\"} 6"));
        assert!(metrics.contains("proxy_redis_ready 1"));
        assert!(metrics.contains("proxy_warp_instances_configured 3"));
        assert!(metrics.contains("proxy_xray_active_nodes 5"));
    }
}
