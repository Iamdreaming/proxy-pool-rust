//! proxy-mcp: MCP Server for proxy pool management.
//!
//! Provides MCP tools that allow LLMs to interact with the proxy pool:
//! - Get/check proxies
//! - Pool status and stats
//! - Refresh and manage the pool
//! - GeoIP lookups
//! - WARP status
//!
//! Supports both stdio and Streamable HTTP transports.

use proxy_core::geoip::GeoIPLookup;
use proxy_core::scheduler::SchedulerHandle;
use proxy_core::store::ProxyStore;
use proxy_core::warp::balancer::WarpBalancer;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::tool_handler;
use rmcp::{ServerHandler, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// Tool parameter structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProtocolParam {
    pub protocol: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListProxiesParam {
    pub protocol: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckProxyParam {
    pub host: String,
    pub port: u16,
    pub protocol: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GeoipLookupParam {
    pub host: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveProxyParam {
    pub host: String,
    pub port: u16,
    pub protocol: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Serialize a `serde_json::Value` to a pretty-printed string.
///
/// `serde_json::to_string_pretty` on a `Value` is infallible, so we use
/// `expect` instead of `unwrap_or_default` to make that clear.
fn to_json(value: serde_json::Value) -> String {
    serde_json::to_string_pretty(&value).expect("infallible: Value serialization")
}

// ---------------------------------------------------------------------------
// MCP Server implementation
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ProxyPoolMcp {
    store: Arc<ProxyStore>,
    balancer: Option<Arc<WarpBalancer>>,
    geoip: Option<Arc<Mutex<GeoIPLookup>>>,
    scheduler_handle: SchedulerHandle,
    tool_router: ToolRouter<Self>,
}

impl ProxyPoolMcp {
    pub fn new(
        store: Arc<ProxyStore>,
        balancer: Option<Arc<WarpBalancer>>,
        geoip: Option<Arc<Mutex<GeoIPLookup>>>,
        scheduler_handle: SchedulerHandle,
    ) -> Self {
        Self {
            store,
            balancer,
            geoip,
            scheduler_handle,
            tool_router: Self::tool_router(),
        }
    }

    fn resolve_protocol(&self, protocol: Option<&str>) -> proxy_core::models::Protocol {
        protocol
            .and_then(proxy_core::models::Protocol::from_str_loose)
            .unwrap_or(proxy_core::models::Protocol::Http)
    }
}

#[tool_router(router = tool_router)]
impl ProxyPoolMcp {
    #[tool(
        description = "Get a random working proxy from the pool. Optionally specify protocol: http, https, socks4, socks5"
    )]
    async fn get_proxy(&self, params: Parameters<ProtocolParam>) -> Result<String, String> {
        let protocol = params.0.protocol;
        let proto = self.resolve_protocol(protocol.as_deref());
        match self.store.get_random(proto).await {
            Ok(Some(proxy)) => Ok(to_json(serde_json::to_value(&proxy).unwrap_or_default())),
            Ok(None) => Ok("No proxy available for the requested protocol".into()),
            Err(e) => Err(format!("Error: {e}")),
        }
    }

    #[tool(description = "Get the best (highest scored) proxy from the pool")]
    async fn get_best_proxy(&self, params: Parameters<ProtocolParam>) -> Result<String, String> {
        let protocol = params.0.protocol;
        let proto = self.resolve_protocol(protocol.as_deref());
        match self.store.get_best(proto).await {
            Ok(Some(proxy)) => Ok(to_json(serde_json::to_value(&proxy).unwrap_or_default())),
            Ok(None) => Ok("No proxy available".into()),
            Err(e) => Err(format!("Error: {e}")),
        }
    }

    #[tool(description = "List proxies in the pool with optional protocol filter and limit")]
    async fn list_proxies(&self, params: Parameters<ListProxiesParam>) -> Result<String, String> {
        let protocol = params.0.protocol;
        let limit = params.0.limit.unwrap_or(20);
        let proto = self.resolve_protocol(protocol.as_deref());
        match self.store.all(proto).await {
            Ok(all) => {
                let proxies: Vec<_> = all.into_iter().take(limit).collect();
                Ok(to_json(serde_json::json!({ "proxies": proxies })))
            }
            Err(e) => Err(format!("Error: {e}")),
        }
    }

    #[tool(description = "Check if a specific proxy is working by testing connectivity")]
    async fn check_proxy(&self, params: Parameters<CheckProxyParam>) -> String {
        let host = &params.0.host;
        let port = params.0.port;
        let protocol = &params.0.protocol;
        let proto = proxy_core::models::Protocol::from_str_loose(protocol)
            .unwrap_or(proxy_core::models::Protocol::Http);
        let proxy = proxy_core::models::Proxy::new(host, port, proto);
        let validator = proxy_core::validator::Validator::new("https://httpbin.org/ip", 10);

        match validator.validate_one(&proxy).await {
            Some(alive) => to_json(serde_json::json!({
                "alive": true,
                "latency_ms": alive.latency_ms,
                "anonymity": alive.anonymity.map(|a| a.to_string()),
            })),
            None => to_json(serde_json::json!({
                "alive": false,
                "host": host,
                "port": port,
                "protocol": protocol,
            })),
        }
    }

    #[tool(description = "Get the current status of the proxy pool")]
    async fn pool_status(&self) -> String {
        let http_count = self
            .store
            .count(proxy_core::models::Protocol::Http)
            .await
            .unwrap_or(0);
        let https_count = self
            .store
            .count(proxy_core::models::Protocol::Https)
            .await
            .unwrap_or(0);
        let socks5_count = self
            .store
            .count(proxy_core::models::Protocol::Socks5)
            .await
            .unwrap_or(0);

        to_json(serde_json::json!({
            "pool": {
                "http": http_count,
                "https": https_count,
                "socks5": socks5_count,
                "total": http_count + https_count + socks5_count,
            }
        }))
    }

    #[tool(description = "Get the status of WARP instances")]
    async fn warp_status(&self) -> String {
        match &self.balancer {
            Some(balancer) => {
                let healthy = balancer.healthy_list().await;
                to_json(serde_json::json!({
                    "warp": {
                        "healthy_count": healthy.len(),
                        "instances": healthy,
                    }
                }))
            }
            None => "WARP not configured".into(),
        }
    }

    #[tool(description = "Look up the geographic location of a host (IP or domain)")]
    async fn geoip_lookup(&self, params: Parameters<GeoipLookupParam>) -> String {
        match &self.geoip {
            Some(geoip) => {
                let mut geoip = geoip.lock().await;
                let info = geoip.lookup(&params.0.host).await;
                to_json(serde_json::json!({
                    "host": params.0.host,
                    "country": info.country,
                    "country_name": info.country_name,
                    "is_overseas": geoip.is_overseas(&info.country),
                }))
            }
            None => "GeoIP not configured (set geoip.database_path in config)".into(),
        }
    }

    #[tool(description = "Remove a proxy from the pool")]
    async fn remove_proxy(&self, params: Parameters<RemoveProxyParam>) -> Result<String, String> {
        let proto = proxy_core::models::Protocol::from_str_loose(&params.0.protocol)
            .unwrap_or(proxy_core::models::Protocol::Http);
        let proxy = proxy_core::models::Proxy::new(&params.0.host, params.0.port, proto);
        match self.store.mark_failed(&proxy).await {
            Ok(()) => Ok(format!("Proxy {}:{} removed", params.0.host, params.0.port)),
            Err(e) => Err(format!("Error: {e}")),
        }
    }

    #[tool(description = "Trigger a pool refresh (fetch new proxies + validate)")]
    async fn refresh_pool(&self) -> String {
        match self.scheduler_handle.refresh().await {
            Ok(result) => to_json(serde_json::json!({
                "status": "ok",
                "fetched": result.fetched,
                "validated": result.validated,
                "stored": result.stored,
                "errors": result.errors,
            })),
            Err(e) => to_json(serde_json::json!({
                "status": "error",
                "message": format!("{e}"),
            })),
        }
    }

    #[tool(description = "Get proxy pool statistics (protocol distribution)")]
    async fn proxy_stats(&self) -> String {
        let mut stats = serde_json::json!({});
        for proto in proxy_core::models::Protocol::all() {
            let count = self.store.count(*proto).await.unwrap_or(0);
            stats[&proto.to_string()] = serde_json::json!(count);
        }
        to_json(serde_json::json!({
            "protocol_distribution": stats,
        }))
    }

    #[tool(description = "Update the proxy-pool service by pulling the latest Docker image and restarting the container. Requires Docker socket access.")]
    async fn update_service(&self) -> String {
        // Step 1: Get current image digest
        let current_digest = match tokio::process::Command::new("docker")
            .args(["inspect", "--format", "{{.Image}}", "proxy-pool"])
            .output()
            .await
        {
            Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
            Err(e) => format!("error inspecting container: {e}"),
        };

        // Step 2: Pull latest image
        let pull_result = tokio::process::Command::new("docker")
            .args(["pull", "ghcr.io/iamdreaming/proxy-pool-rust:latest"])
            .output()
            .await;

        let pull_output = match pull_result {
            Ok(out) => {
                if out.status.success() {
                    String::from_utf8_lossy(&out.stdout).to_string()
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    return to_json(serde_json::json!({
                        "status": "error",
                        "message": format!("docker pull failed: {stderr}"),
                        "current_digest": current_digest,
                    }));
                }
            }
            Err(e) => {
                return to_json(serde_json::json!({
                    "status": "error",
                    "message": format!("docker pull command failed: {e}"),
                    "current_digest": current_digest,
                }));
            }
        };

        // Step 3: Restart with new image
        let restart_result = tokio::process::Command::new("docker")
            .args(["compose", "-f", "/opt/proxy-pool/deploy/docker-compose.yml", "up", "-d", "proxy-pool"])
            .output()
            .await;

        match restart_result {
            Ok(out) if out.status.success() => {
                let new_digest = match tokio::process::Command::new("docker")
                    .args(["inspect", "--format", "{{.Image}}", "proxy-pool"])
                    .output()
                    .await
                {
                    Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
                    Err(_) => "unknown".to_string(),
                };

                to_json(serde_json::json!({
                    "status": "ok",
                    "previous_digest": current_digest,
                    "new_digest": new_digest,
                    "pull_output": pull_output.lines().last().unwrap_or("").trim(),
                }))
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                to_json(serde_json::json!({
                    "status": "error",
                    "message": format!("docker compose up failed: {stderr}"),
                    "current_digest": current_digest,
                    "pull_output": pull_output.lines().last().unwrap_or("").trim(),
                }))
            }
            Err(e) => {
                to_json(serde_json::json!({
                    "status": "error",
                    "message": format!("docker compose command failed: {e}"),
                    "current_digest": current_digest,
                }))
            }
        }
    }
}

// Implement ServerHandler with tool_handler to enable .serve()
#[tool_handler]
impl ServerHandler for ProxyPoolMcp {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo {
            instructions: Some(
                "Proxy pool management MCP server. Use tools to get, check, and manage proxies."
                    .into(),
            ),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxy_core::scheduler::{SchedulerCommand, SchedulerHandle};
    use tokio::sync::mpsc;

    #[test]
    fn test_protocol_param_deserialize() {
        let json = r#"{"protocol":"socks5"}"#;
        let param: ProtocolParam = serde_json::from_str(json).unwrap();
        assert_eq!(param.protocol.as_deref(), Some("socks5"));
    }

    #[test]
    fn test_protocol_param_optional() {
        let json = r#"{}"#;
        let param: ProtocolParam = serde_json::from_str(json).unwrap();
        assert!(param.protocol.is_none());
    }

    #[test]
    fn test_list_proxies_param_deserialize() {
        let json = r#"{"protocol":"http","limit":10}"#;
        let param: ListProxiesParam = serde_json::from_str(json).unwrap();
        assert_eq!(param.protocol.as_deref(), Some("http"));
        assert_eq!(param.limit, Some(10));
    }

    #[test]
    fn test_check_proxy_param_deserialize() {
        let json = r#"{"host":"1.2.3.4","port":8080,"protocol":"http"}"#;
        let param: CheckProxyParam = serde_json::from_str(json).unwrap();
        assert_eq!(param.host, "1.2.3.4");
        assert_eq!(param.port, 8080);
        assert_eq!(param.protocol, "http");
    }

    #[test]
    fn test_geoip_lookup_param_deserialize() {
        let json = r#"{"host":"google.com"}"#;
        let param: GeoipLookupParam = serde_json::from_str(json).unwrap();
        assert_eq!(param.host, "google.com");
    }

    #[test]
    fn test_remove_proxy_param_deserialize() {
        let json = r#"{"host":"1.2.3.4","port":8080,"protocol":"socks5"}"#;
        let param: RemoveProxyParam = serde_json::from_str(json).unwrap();
        assert_eq!(param.host, "1.2.3.4");
        assert_eq!(param.port, 8080);
        assert_eq!(param.protocol, "socks5");
    }

    #[test]
    fn test_scheduler_handle_clone() {
        let (cmd_tx, _cmd_rx) = mpsc::channel::<SchedulerCommand>(8);
        let handle = SchedulerHandle::new(cmd_tx);
        let _handle2 = handle.clone();
    }
}
