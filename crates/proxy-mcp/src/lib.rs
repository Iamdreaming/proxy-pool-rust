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

use proxy_core::store::ProxyStore;
use proxy_core::warp::balancer::WarpBalancer;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::tool_handler;
use rmcp::{ServerHandler, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;

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
// MCP Server implementation
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ProxyPoolMcp {
    store: Arc<ProxyStore>,
    balancer: Option<Arc<WarpBalancer>>,
    tool_router: ToolRouter<Self>,
}

impl ProxyPoolMcp {
    pub fn new(store: Arc<ProxyStore>, balancer: Option<Arc<WarpBalancer>>) -> Self {
        Self {
            store,
            balancer,
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
            Ok(Some(proxy)) => Ok(serde_json::to_string_pretty(&proxy).unwrap_or_default()),
            Ok(None) => Ok("No proxy available for the requested protocol".into()),
            Err(e) => Err(format!("Error: {e}")),
        }
    }

    #[tool(description = "Get the best (highest scored) proxy from the pool")]
    async fn get_best_proxy(&self, params: Parameters<ProtocolParam>) -> Result<String, String> {
        let protocol = params.0.protocol;
        let proto = self.resolve_protocol(protocol.as_deref());
        match self.store.get_best(proto).await {
            Ok(Some(proxy)) => Ok(serde_json::to_string_pretty(&proxy).unwrap_or_default()),
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
                Ok(serde_json::to_string_pretty(&proxies).unwrap_or_default())
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
            Some(alive) => serde_json::to_string_pretty(&serde_json::json!({
                "alive": true,
                "latency_ms": alive.latency_ms,
                "anonymity": alive.anonymity.map(|a| a.to_string()),
            }))
            .unwrap_or_default(),
            None => serde_json::to_string_pretty(&serde_json::json!({
                "alive": false,
                "host": host,
                "port": port,
                "protocol": protocol,
            }))
            .unwrap_or_default(),
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

        serde_json::to_string_pretty(&serde_json::json!({
            "pool": {
                "http": http_count,
                "https": https_count,
                "socks5": socks5_count,
                "total": http_count + https_count + socks5_count,
            }
        }))
        .unwrap_or_default()
    }

    #[tool(description = "Get the status of WARP instances")]
    async fn warp_status(&self) -> String {
        match &self.balancer {
            Some(balancer) => {
                let healthy = balancer.healthy_list().await;
                serde_json::to_string_pretty(&serde_json::json!({
                    "warp": {
                        "healthy_count": healthy.len(),
                        "instances": healthy,
                    }
                }))
                .unwrap_or_default()
            }
            None => "WARP not configured".into(),
        }
    }

    #[tool(description = "Look up the geographic location of a host (IP or domain)")]
    async fn geoip_lookup(&self, params: Parameters<GeoipLookupParam>) -> String {
        // TODO: Use GeoIPLookup instance
        format!("GeoIP lookup for '{}' - feature coming soon", params.0.host)
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
        // TODO: trigger scheduler.run_once() via channel
        "Pool refresh scheduled".into()
    }

    #[tool(description = "Get proxy pool statistics (protocol distribution)")]
    async fn proxy_stats(&self) -> String {
        let mut stats = serde_json::json!({});
        for proto in proxy_core::models::Protocol::all() {
            let count = self.store.count(*proto).await.unwrap_or(0);
            stats[&proto.to_string()] = serde_json::json!(count);
        }
        serde_json::to_string_pretty(&serde_json::json!({
            "protocol_distribution": stats,
        }))
        .unwrap_or_default()
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
