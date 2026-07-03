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
        let validator = proxy_core::validator::Validator::new(
            "https://www.cloudflare.com/cdn-cgi/trace",
            10,
        );

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

    #[tool(description = "Update the proxy-pool service by pulling the latest Docker image and restarting the container. Uses Docker Engine API via Unix socket (no docker CLI required).")]
    async fn update_service(&self) -> String {
        let socket_path = "/var/run/docker.sock";

        // Step 1: Get current container image digest
        let current_digest = match docker_api_get(socket_path, "/containers/proxy-pool/json").await {
            Ok(body) => body
                .get("Image")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            Err(e) => {
                return to_json(serde_json::json!({
                    "status": "error",
                    "message": format!("failed to inspect container: {e}"),
                }));
            }
        };

        // Step 2: Pull latest image (streaming, wait for completion)
        let image = "ghcr.io/iamdreaming/proxy-pool-rust:latest";
        match docker_api_post(socket_path, &format!("/images/create?fromImage={image}&tag=latest")).await {
            Ok(_) => {}
            Err(e) => {
                return to_json(serde_json::json!({
                    "status": "error",
                    "message": format!("docker pull failed: {e}"),
                    "current_digest": current_digest,
                }));
            }
        };

        // Step 3: Restart container
        match docker_api_post(socket_path, "/containers/proxy-pool/restart").await {
            Ok(_) => {
                // Step 4: Get new digest after restart
                let new_digest = match docker_api_get(socket_path, "/containers/proxy-pool/json").await {
                    Ok(body) => body
                        .get("Image")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    Err(_) => "unknown".to_string(),
                };

                let updated = current_digest != new_digest;
                to_json(serde_json::json!({
                    "status": if updated { "updated" } else { "restarted" },
                    "previous_digest": current_digest,
                    "new_digest": new_digest,
                    "image": image,
                }))
            }
            Err(e) => to_json(serde_json::json!({
                "status": "error",
                "message": format!("container restart failed: {e}"),
                "current_digest": current_digest,
            })),
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

// ---------------------------------------------------------------------------
// Docker Engine API helpers (Unix socket, no docker CLI required)
// Only available on Unix (Linux/macOS) where Docker socket exists.
// ---------------------------------------------------------------------------

#[cfg(unix)]
/// Send a GET request to the Docker Engine API via Unix socket.
/// Returns the parsed JSON response body.
async fn docker_api_get(socket_path: &str, path: &str) -> Result<serde_json::Value, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path)
        .await
        .map_err(|e| format!("connect to {socket_path}: {e}"))?;

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| format!("write: {e}"))?;

    let mut buf = Vec::with_capacity(4096);
    stream
        .read_to_end(&mut buf)
        .await
        .map_err(|e| format!("read: {e}"))?;

    parse_docker_response(&buf)
}

#[cfg(unix)]
/// Send a POST request to the Docker Engine API via Unix socket.
/// Returns the parsed JSON response body (for non-streaming endpoints).
/// For streaming endpoints (like /images/create), waits for completion and
/// returns the last JSON status object.
async fn docker_api_post(socket_path: &str, path: &str) -> Result<serde_json::Value, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path)
        .await
        .map_err(|e| format!("connect to {socket_path}: {e}"))?;

    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nContent-Length: 0\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| format!("write: {e}"))?;

    let mut buf = Vec::with_capacity(8192);
    stream
        .read_to_end(&mut buf)
        .await
        .map_err(|e| format!("read: {e}"))?;

    parse_docker_response(&buf)
}

#[cfg(not(unix))]
async fn docker_api_get(_socket_path: &str, _path: &str) -> Result<serde_json::Value, String> {
    Err("Docker Engine API is only available on Unix (requires Unix socket)".into())
}

#[cfg(not(unix))]
async fn docker_api_post(_socket_path: &str, _path: &str) -> Result<serde_json::Value, String> {
    Err("Docker Engine API is only available on Unix (requires Unix socket)".into())
}

/// Parse an HTTP response from the Docker Engine API.
/// Extracts the body (handling chunked transfer-encoding) and parses as JSON.
#[cfg(unix)]
fn parse_docker_response(buf: &[u8]) -> Result<serde_json::Value, String> {
    let text = String::from_utf8_lossy(buf);

    // Split headers from body
    let (header_part, body_part) = text
        .find("\r\n\r\n")
        .map(|pos| (&text[..pos], &text[pos + 4..]))
        .ok_or("no HTTP header/body separator")?;

    // Check status line
    let status_line = header_part.lines().next().unwrap_or("");
    if !status_line.contains("200") && !status_line.contains("201") && !status_line.contains("204") {
        return Err(format!("HTTP error: {status_line}"));
    }

    // Handle chunked transfer-encoding
    let body = if header_part.contains("chunked") {
        decode_chunked(body_part)
    } else {
        body_part.to_string()
    };

    if body.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }

    // For streaming responses (like /images/create), multiple JSON objects
    // are newline-delimited. Return the last one.
    let lines: Vec<&str> = body.trim().lines().filter(|l| !l.trim().is_empty()).collect();
    let last_line = lines.last().unwrap_or(&"");

    serde_json::from_str(last_line).map_err(|e| format!("JSON parse error: {e}, body: {}", &body[..body.len().min(200)]))
}

/// Decode a chunked transfer-encoding body.
#[cfg(unix)]
fn decode_chunked(body: &str) -> String {
    let mut result = String::new();
    let mut remaining = body;

    while let Some(line_end) = remaining.find("\r\n") {
        let size_str = &remaining[..line_end];
        let chunk_size = match usize::from_str_radix(size_str.trim(), 16) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };

        let data_start = line_end + 2;
        let data_end = data_start + chunk_size.min(remaining.len() - data_start);
        result.push_str(&remaining[data_start..data_end]);

        // Skip chunk data + trailing \r\n
        if data_end + 2 <= remaining.len() {
            remaining = &remaining[data_end + 2..];
        } else {
            break;
        }
    }

    result
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
