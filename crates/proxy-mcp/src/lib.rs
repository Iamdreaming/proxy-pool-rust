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
use proxy_core::models::ProxyFilter;
use proxy_core::route_debug::UpstreamSelector;
use proxy_core::scheduler::SchedulerHandle;
use proxy_core::status::collect_service_status;
use proxy_core::store::ProxyStore;
use proxy_core::warp::balancer::WarpBalancer;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::tool_handler;
use rmcp::{ServerHandler, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// Tool parameter structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProxyFilterParam {
    pub protocol: Option<String>,
    /// ISO country code (e.g. "US", "JP"). Exact match.
    pub country: Option<String>,
    /// Minimum anonymity level: "transparent", "anonymous", or "elite".
    pub anonymity: Option<String>,
    /// Maximum acceptable latency in milliseconds.
    pub max_latency: Option<f64>,
    /// `true` = overseas only, `false` = domestic only.
    pub overseas: Option<bool>,
    /// Minimum composite score (0.0..1.0).
    pub min_score: Option<f64>,
    /// Filter by source name (exact match).
    pub source: Option<String>,
    /// `true` = exclude circuit-open proxies.
    pub alive: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListProxiesParam {
    pub protocol: Option<String>,
    pub limit: Option<usize>,
    /// ISO country code (e.g. "US", "JP"). Exact match.
    pub country: Option<String>,
    /// Minimum anonymity level: "transparent", "anonymous", or "elite".
    pub anonymity: Option<String>,
    /// Maximum acceptable latency in milliseconds.
    pub max_latency: Option<f64>,
    /// `true` = overseas only, `false` = domestic only.
    pub overseas: Option<bool>,
    /// Minimum composite score (0.0..1.0).
    pub min_score: Option<f64>,
    /// Filter by source name (exact match).
    pub source: Option<String>,
    /// `true` = exclude circuit-open proxies.
    pub alive: Option<bool>,
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RefreshFetcherParam {
    /// Stable fetcher id, such as "proxyscrape:http" or "geonode".
    pub fetcher: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RouteTestParam {
    /// Target host to evaluate, such as "github.com".
    pub host: String,
    /// Optional protocol for pool lookup: http, https, socks4, socks5.
    pub protocol: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CleanupLowScoreParam {
    /// Optional protocol to scan: http, https, socks4, socks5. Defaults to http.
    pub protocol: Option<String>,
    /// Maximum number of stored proxies to scan. Defaults to 100.
    pub limit: Option<usize>,
    /// Optional min score override. Defaults to the store configured min_score.
    pub min_score: Option<f64>,
    /// Apply removals. Defaults to false, which performs a dry-run.
    pub apply: Option<bool>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateServiceConfig {
    enabled: bool,
    socket_path: String,
    container_name: String,
    image: String,
    image_repo: String,
    image_tag: String,
    watchtower_url: String,
    watchtower_token: Option<String>,
}

impl UpdateServiceConfig {
    fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    fn from_lookup(mut get: impl FnMut(&str) -> Option<String>) -> Self {
        let image = get("PROXY_POOL_UPDATE_IMAGE")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "ghcr.io/iamdreaming/proxy-pool-rust:latest".into());
        let (image_repo, image_tag) = split_image_ref(&image);

        Self {
            enabled: parse_bool_env(get("PROXY_POOL_UPDATE_ENABLED").as_deref()),
            socket_path: get("PROXY_POOL_UPDATE_DOCKER_SOCKET")
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "/var/run/docker.sock".into()),
            container_name: get("PROXY_POOL_UPDATE_CONTAINER")
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "proxy-pool".into()),
            image,
            image_repo,
            image_tag,
            watchtower_url: get("PROXY_POOL_UPDATE_WATCHTOWER_URL")
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "http://watchtower-proxy-pool:8080/v1/update".into()),
            watchtower_token: get("PROXY_POOL_UPDATE_TOKEN")
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        }
    }
}

fn parse_bool_env(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()),
        Some(v) if matches!(v.as_str(), "1" | "true" | "yes" | "on")
    )
}

fn split_image_ref(image: &str) -> (String, String) {
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

// ---------------------------------------------------------------------------
// MCP Server implementation
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ProxyPoolMcp {
    store: Arc<ProxyStore>,
    balancer: Option<Arc<WarpBalancer>>,
    geoip: Option<Arc<Mutex<GeoIPLookup>>>,
    scheduler_handle: SchedulerHandle,
    route_selector: Arc<UpstreamSelector>,
    xray_active_count: Arc<AtomicUsize>,
    git_hash: &'static str,
    started_at: Instant,
    tool_router: ToolRouter<Self>,
}

/// Dependencies required to construct the MCP service.
pub struct ProxyPoolMcpConfig {
    pub store: Arc<ProxyStore>,
    pub balancer: Option<Arc<WarpBalancer>>,
    pub geoip: Option<Arc<Mutex<GeoIPLookup>>>,
    pub scheduler_handle: SchedulerHandle,
    pub route_selector: Arc<UpstreamSelector>,
    pub xray_active_count: Arc<AtomicUsize>,
    pub git_hash: &'static str,
    pub started_at: Instant,
}

impl ProxyPoolMcp {
    pub fn new(config: ProxyPoolMcpConfig) -> Self {
        Self {
            store: config.store,
            balancer: config.balancer,
            geoip: config.geoip,
            scheduler_handle: config.scheduler_handle,
            route_selector: config.route_selector,
            xray_active_count: config.xray_active_count,
            git_hash: config.git_hash,
            started_at: config.started_at,
            tool_router: Self::tool_router(),
        }
    }

    fn resolve_protocol(&self, protocol: Option<&str>) -> proxy_core::models::Protocol {
        protocol
            .and_then(proxy_core::models::Protocol::from_str_loose)
            .unwrap_or(proxy_core::models::Protocol::Http)
    }

    fn to_filter(param: &ProxyFilterParam) -> ProxyFilter {
        ProxyFilter {
            country: param.country.clone(),
            anonymity: param.anonymity.clone(),
            max_latency: param.max_latency,
            overseas: param.overseas,
            min_score: param.min_score,
            source: param.source.clone(),
            alive: param.alive,
        }
    }

    fn to_filter_from_list(param: &ListProxiesParam) -> ProxyFilter {
        ProxyFilter {
            country: param.country.clone(),
            anonymity: param.anonymity.clone(),
            max_latency: param.max_latency,
            overseas: param.overseas,
            min_score: param.min_score,
            source: param.source.clone(),
            alive: param.alive,
        }
    }
}

#[tool_router(router = tool_router)]
impl ProxyPoolMcp {
    #[tool(description = "Get a random working proxy from the pool. \
        Optionally specify protocol (http, https, socks4, socks5) \
        and filter by country, anonymity, max_latency, overseas, min_score, source, alive.")]
    async fn get_proxy(&self, params: Parameters<ProxyFilterParam>) -> Result<String, String> {
        let filter = Self::to_filter(&params.0);
        let proto = self.resolve_protocol(params.0.protocol.as_deref());
        match self.store.get_random_filtered(proto, &filter).await {
            Ok(Some(proxy)) => Ok(to_json(serde_json::to_value(&proxy).unwrap_or_default())),
            Ok(None) => Ok("No proxy available matching the filter criteria".into()),
            Err(e) => Err(format!("Error: {e}")),
        }
    }

    #[tool(description = "Get the best (highest scored) proxy from the pool. \
        Optionally filter by country, anonymity, max_latency, overseas, min_score, source, alive.")]
    async fn get_best_proxy(&self, params: Parameters<ProxyFilterParam>) -> Result<String, String> {
        let filter = Self::to_filter(&params.0);
        let proto = self.resolve_protocol(params.0.protocol.as_deref());
        match self.store.get_best_filtered(proto, &filter).await {
            Ok(Some(proxy)) => Ok(to_json(serde_json::to_value(&proxy).unwrap_or_default())),
            Ok(None) => Ok("No proxy available matching the filter criteria".into()),
            Err(e) => Err(format!("Error: {e}")),
        }
    }

    #[tool(
        description = "List proxies in the pool with optional protocol filter, limit, \
        and advanced filters: country, anonymity, max_latency, overseas, min_score, source, alive."
    )]
    async fn list_proxies(&self, params: Parameters<ListProxiesParam>) -> Result<String, String> {
        let filter = Self::to_filter_from_list(&params.0);
        let limit = params.0.limit.unwrap_or(20);
        let proto = self.resolve_protocol(params.0.protocol.as_deref());
        match self.store.query(proto, &filter, limit).await {
            Ok(proxies) => Ok(to_json(serde_json::json!({
                "count": proxies.len(),
                "proxies": proxies,
            }))),
            Err(e) => Err(format!("Error: {e}")),
        }
    }

    #[tool(
        description = "Explain proxy scores in the pool. Optionally specify protocol (http, https, socks4, socks5), limit, country, anonymity, max_latency, overseas, min_score, source, and alive."
    )]
    async fn explain_proxy_scores(
        &self,
        params: Parameters<ListProxiesParam>,
    ) -> Result<String, String> {
        let filter = Self::to_filter_from_list(&params.0);
        let limit = params.0.limit.unwrap_or(20);
        let proto = self.resolve_protocol(params.0.protocol.as_deref());
        match self.store.query_scored(proto, &filter, limit).await {
            Ok(proxies) => Ok(to_json(serde_json::json!({
                "count": proxies.len(),
                "proxies": proxies,
            }))),
            Err(e) => Err(format!("Error: {e}")),
        }
    }

    #[tool(
        description = "Dry-run or apply cleanup of low-score proxies. Optionally specify protocol (http, https, socks4, socks5), limit, min_score, and apply. Defaults to dry-run with apply=false."
    )]
    async fn cleanup_low_score_proxies(
        &self,
        params: Parameters<CleanupLowScoreParam>,
    ) -> Result<String, String> {
        let proto = self.resolve_protocol(params.0.protocol.as_deref());
        let limit = params.0.limit.unwrap_or(100);
        let apply = params.0.apply.unwrap_or(false);
        match self
            .store
            .cleanup_low_score(proto, limit, params.0.min_score, apply)
            .await
        {
            Ok(result) => Ok(to_json(serde_json::to_value(result).unwrap_or_default())),
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
        let validator =
            proxy_core::validator::Validator::new("https://www.cloudflare.com/cdn-cgi/trace", 10);

        to_json(serde_json::to_value(validator.check_one(&proxy).await).unwrap_or_default())
    }

    #[tool(
        description = "Get structured service status, including version, uptime, Redis, pool, WARP, and xray summaries"
    )]
    async fn service_status(&self) -> String {
        let xray_active = self.xray_active_count.load(Ordering::Relaxed);
        let status = collect_service_status(
            &self.store,
            self.balancer.as_deref(),
            env!("CARGO_PKG_VERSION"),
            self.git_hash,
            self.started_at.elapsed().as_secs(),
            xray_active,
        )
        .await;
        serde_json::to_string_pretty(&status).unwrap_or_default()
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
                "fetchers": result.fetchers,
            })),
            Err(e) => to_json(serde_json::json!({
                "status": "error",
                "message": format!("{e}"),
            })),
        }
    }

    #[tool(description = "Get the latest status report for each configured proxy fetcher")]
    async fn fetcher_status(&self) -> String {
        let fetchers = self.scheduler_handle.fetcher_statuses().await;
        to_json(serde_json::json!({
            "fetchers": fetchers,
        }))
    }

    #[tool(
        description = "Refresh one configured proxy fetcher by id, such as proxyscrape:http or geonode"
    )]
    async fn refresh_fetcher(&self, params: Parameters<RefreshFetcherParam>) -> String {
        match self
            .scheduler_handle
            .refresh_fetcher(params.0.fetcher.clone())
            .await
        {
            Ok(result) => to_json(serde_json::json!({
                "status": "ok",
                "fetched": result.fetched,
                "validated": result.validated,
                "stored": result.stored,
                "errors": result.errors,
                "fetchers": result.fetchers,
            })),
            Err(e) => to_json(serde_json::json!({
                "status": "error",
                "message": format!("{e}"),
            })),
        }
    }

    #[tool(
        description = "Test gateway route selection for a host. Optionally specify protocol: http, https, socks4, socks5"
    )]
    async fn route_test(&self, params: Parameters<RouteTestParam>) -> String {
        let protocol = self
            .resolve_protocol(params.0.protocol.as_deref())
            .to_string();
        let decision = self.route_selector.dry_run(&params.0.host, &protocol).await;
        to_json(serde_json::json!({
            "status": "ok",
            "decision": decision,
        }))
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

    #[tool(
        description = "Update the proxy-pool service by pulling the configured Docker image and triggering Watchtower. Requires PROXY_POOL_UPDATE_ENABLED=true."
    )]
    async fn update_service(&self) -> String {
        let config = UpdateServiceConfig::from_env();
        if !config.enabled {
            return to_json(serde_json::json!({
                "status": "disabled",
                "message": "update_service is disabled; set PROXY_POOL_UPDATE_ENABLED=true to allow Docker/Watchtower updates",
                "required_env": "PROXY_POOL_UPDATE_ENABLED=true",
            }));
        }

        let Some(watchtower_token) = config.watchtower_token.as_deref() else {
            return to_json(serde_json::json!({
                "status": "error",
                "message": "PROXY_POOL_UPDATE_TOKEN must be set when update_service is enabled",
            }));
        };

        // Step 1: Inspect current container to get previous image identity.
        tracing::info!(
            container = %config.container_name,
            "update_service: inspecting current container"
        );
        let old_inspect = match docker_api_get(
            &config.socket_path,
            &format!("/containers/{}/json", config.container_name),
        )
        .await
        {
            Ok(body) => body,
            Err(e) => {
                return to_json(serde_json::json!({
                    "status": "error",
                    "message": format!("failed to inspect container: {e}"),
                }));
            }
        };

        let previous_image_id = old_inspect
            .get("Image")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Step 2: Pull latest image (pre-fetch so Watchtower doesn't need to)
        tracing::info!(image = %config.image, "update_service: pulling image");
        if let Err(e) = docker_api_post(
            &config.socket_path,
            &format!(
                "/images/create?fromImage={}&tag={}",
                docker_api_escape(&config.image_repo),
                docker_api_escape(&config.image_tag)
            ),
        )
        .await
        {
            return to_json(serde_json::json!({
                "status": "error",
                "message": format!("docker pull failed: {e}"),
                "previous_image_id": previous_image_id,
            }));
        }

        let new_inspect = match docker_api_get(
            &config.socket_path,
            &format!("/images/{}/json", docker_api_escape(&config.image)),
        )
        .await
        {
            Ok(body) => body,
            Err(e) => {
                return to_json(serde_json::json!({
                    "status": "error",
                    "message": format!("failed to inspect pulled image: {e}"),
                    "previous_image_id": previous_image_id,
                    "image": config.image,
                }));
            }
        };
        let new_digest = docker_image_digest(&new_inspect).unwrap_or_else(|| "unknown".into());
        let new_image_id = docker_image_id(&new_inspect).unwrap_or_else(|| "unknown".into());
        let digest_changed = image_identity_changed(&previous_image_id, &new_image_id);

        if !digest_changed {
            return to_json(serde_json::json!({
                "status": "already_current",
                "previous_image_id": previous_image_id,
                "new_image_id": new_image_id,
                "new_digest": new_digest,
                "digest_changed": false,
                "image": config.image,
                "message": "Pulled image matches the running container image; Watchtower was not triggered.",
            }));
        }

        // Step 3: Trigger Watchtower to update the container
        // Watchtower is an independent container that handles stop/recreate/start
        // safely — it doesn't have the "self-surgery" problem.
        tracing::info!("update_service: triggering Watchtower update");
        let client = reqwest::Client::new();
        let resp = client
            .post(&config.watchtower_url)
            .header("Authorization", format!("Bearer {watchtower_token}"))
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                tracing::info!("update_service: Watchtower update triggered successfully");
                // Note: the current container will be stopped and recreated by Watchtower.
                // This process will be killed, so the response may not reach the caller.
                // The success signal is the new container's git_hash changing (verified externally).
                to_json(serde_json::json!({
                    "status": "update_triggered",
                    "previous_image_id": previous_image_id,
                    "new_image_id": new_image_id,
                    "new_digest": new_digest,
                    "digest_changed": digest_changed,
                    "image": config.image,
                    "message": "Watchtower update triggered. The container will be recreated shortly.",
                }))
            }
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                to_json(serde_json::json!({
                    "status": "error",
                    "message": format!("Watchtower returned {status}: {body}"),
                    "previous_image_id": previous_image_id,
                    "new_image_id": new_image_id,
                    "new_digest": new_digest,
                    "digest_changed": digest_changed,
                }))
            }
            Err(e) => to_json(serde_json::json!({
                "status": "error",
                "message": format!("failed to reach Watchtower: {e}"),
                "previous_image_id": previous_image_id,
                "new_image_id": new_image_id,
                "new_digest": new_digest,
                "digest_changed": digest_changed,
            })),
        }
    }
}

// Implement ServerHandler with tool_handler to enable .serve()
#[tool_handler]
impl ServerHandler for ProxyPoolMcp {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo {
            capabilities: rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .build(),
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
/// Read a complete HTTP response from a Unix socket stream.
/// Detects response completion via Content-Length or chunked transfer-encoding,
/// rather than waiting for EOF (which never comes with HTTP/1.1 keep-alive).
async fn read_http_response(
    stream: &mut tokio::net::UnixStream,
    per_read_timeout_secs: u64,
) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncReadExt;
    use tokio::time::{Duration, timeout};

    let mut buf = Vec::with_capacity(8192);
    let mut tmp = [0u8; 8192];
    let max_size = 67_108_864; // 64 MiB

    // Phase 1: Read until we have the full headers (ends with \r\n\r\n)
    loop {
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        match timeout(
            Duration::from_secs(per_read_timeout_secs),
            stream.read(&mut tmp),
        )
        .await
        {
            Ok(Ok(0)) => return Ok(buf), // EOF before headers — let parse_docker_response handle it
            Ok(Ok(n)) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.len() > max_size {
                    return Err("response too large (>64 MiB)".into());
                }
            }
            Ok(Err(e)) => return Err(format!("read: {e}")),
            Err(_) => return Err(format!("read: timed out after {per_read_timeout_secs}s")),
        }
    }

    // Find header/body boundary
    let header_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let header_str = String::from_utf8_lossy(&buf[..header_end]);
    let body_start = header_end + 4;
    let body_received = buf.len().saturating_sub(body_start);

    // Phase 2: Determine how to read the body
    if header_str.contains("chunked") {
        // Chunked: read until we see the terminal chunk "0\r\n\r\n"
        loop {
            let body_part = &buf[body_start..];
            if body_part.windows(5).any(|w| w == b"0\r\n\r\n") {
                break;
            }
            match timeout(
                Duration::from_secs(per_read_timeout_secs),
                stream.read(&mut tmp),
            )
            .await
            {
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => {
                    buf.extend_from_slice(&tmp[..n]);
                    if buf.len() > max_size {
                        return Err("response too large (>64 MiB)".into());
                    }
                }
                Ok(Err(e)) => return Err(format!("read: {e}")),
                Err(_) => return Err(format!("read: timed out after {per_read_timeout_secs}s")),
            }
        }
    } else if let Some(content_length) = extract_content_length(&header_str) {
        // Content-Length: read until we have exactly that many body bytes
        let needed = content_length.saturating_sub(body_received);
        let mut remaining = needed;
        while remaining > 0 {
            match timeout(
                Duration::from_secs(per_read_timeout_secs),
                stream.read(&mut tmp),
            )
            .await
            {
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => {
                    buf.extend_from_slice(&tmp[..n]);
                    remaining = remaining.saturating_sub(n);
                    if buf.len() > max_size {
                        return Err("response too large (>64 MiB)".into());
                    }
                }
                Ok(Err(e)) => return Err(format!("read: {e}")),
                Err(_) => return Err(format!("read: timed out after {per_read_timeout_secs}s")),
            }
        }
    }
    // else: no Content-Length and not chunked — body already in buf from phase 1

    Ok(buf)
}

#[cfg(unix)]
/// Extract Content-Length value from HTTP headers.
fn extract_content_length(header_str: &str) -> Option<usize> {
    for line in header_str.lines() {
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            return rest.trim().parse().ok();
        }
        if let Some(rest) = line.strip_prefix("content-length:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

#[cfg(unix)]
/// Send a GET request to the Docker Engine API via Unix socket.
/// Returns the parsed JSON response body.
async fn docker_api_get(socket_path: &str, path: &str) -> Result<serde_json::Value, String> {
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixStream;
    use tokio::time::{Duration, timeout};

    let mut stream = timeout(Duration::from_secs(10), UnixStream::connect(socket_path))
        .await
        .map_err(|_| format!("connect to {socket_path}: timed out after 10s"))?
        .map_err(|e| format!("connect to {socket_path}: {e}"))?;

    let request =
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\n\r\n");
    timeout(
        Duration::from_secs(10),
        stream.write_all(request.as_bytes()),
    )
    .await
    .map_err(|_| "write: timed out after 10s".to_string())?
    .map_err(|e| format!("write: {e}"))?;

    let buf = read_http_response(&mut stream, 30).await?;
    parse_docker_response(&buf)
}

#[cfg(unix)]
/// Send a POST request to the Docker Engine API via Unix socket.
/// Returns the parsed JSON response body (for non-streaming endpoints).
/// For streaming endpoints (like /images/create), waits for completion and
/// returns the last JSON status object.
async fn docker_api_post(socket_path: &str, path: &str) -> Result<serde_json::Value, String> {
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixStream;
    use tokio::time::{Duration, timeout};

    let mut stream = timeout(Duration::from_secs(10), UnixStream::connect(socket_path))
        .await
        .map_err(|_| format!("connect to {socket_path}: timed out after 10s"))?
        .map_err(|e| format!("connect to {socket_path}: {e}"))?;

    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nContent-Length: 0\r\n\r\n"
    );
    timeout(
        Duration::from_secs(10),
        stream.write_all(request.as_bytes()),
    )
    .await
    .map_err(|_| "write: timed out after 10s".to_string())?
    .map_err(|e| format!("write: {e}"))?;

    let buf = read_http_response(&mut stream, 60).await?;
    parse_docker_response(&buf)
}

#[cfg(unix)]
#[allow(dead_code)]
/// Send a POST request with a JSON body to the Docker Engine API via Unix socket.
/// Returns the parsed JSON response body.
async fn docker_api_post_json(
    socket_path: &str,
    path: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixStream;
    use tokio::time::{Duration, timeout};

    let mut stream = timeout(Duration::from_secs(10), UnixStream::connect(socket_path))
        .await
        .map_err(|_| format!("connect to {socket_path}: timed out after 10s"))?
        .map_err(|e| format!("connect to {socket_path}: {e}"))?;

    let body_str = body.to_string();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body_str.len(),
        body_str,
    );
    timeout(
        Duration::from_secs(10),
        stream.write_all(request.as_bytes()),
    )
    .await
    .map_err(|_| "write: timed out after 10s".to_string())?
    .map_err(|e| format!("write: {e}"))?;

    let buf = read_http_response(&mut stream, 30).await?;
    parse_docker_response(&buf)
}

#[cfg(unix)]
#[allow(dead_code)]
/// Send a DELETE request to the Docker Engine API via Unix socket.
/// Returns the parsed JSON response body.
async fn docker_api_delete(socket_path: &str, path: &str) -> Result<serde_json::Value, String> {
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixStream;
    use tokio::time::{Duration, timeout};

    let mut stream = timeout(Duration::from_secs(10), UnixStream::connect(socket_path))
        .await
        .map_err(|_| format!("connect to {socket_path}: timed out after 10s"))?
        .map_err(|e| format!("connect to {socket_path}: {e}"))?;

    let request =
        format!("DELETE {path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\n\r\n");
    timeout(
        Duration::from_secs(10),
        stream.write_all(request.as_bytes()),
    )
    .await
    .map_err(|_| "write: timed out after 10s".to_string())?
    .map_err(|e| format!("write: {e}"))?;

    let buf = read_http_response(&mut stream, 30).await?;
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

#[cfg(not(unix))]
#[allow(dead_code)]
async fn docker_api_post_json(
    _socket_path: &str,
    _path: &str,
    _body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    Err("Docker Engine API is only available on Unix (requires Unix socket)".into())
}

#[cfg(not(unix))]
#[allow(dead_code)]
async fn docker_api_delete(_socket_path: &str, _path: &str) -> Result<serde_json::Value, String> {
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
    let is_success =
        status_line.contains("200") || status_line.contains("201") || status_line.contains("204");

    // Handle chunked transfer-encoding
    let body = if header_part.contains("chunked") {
        decode_chunked(body_part)
    } else {
        body_part.to_string()
    };

    if !is_success {
        // Include response body in error for diagnostics
        let detail = body.trim();
        return Err(format!("HTTP error: {status_line} — {detail}"));
    };

    if body.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }

    // For streaming responses (like /images/create), multiple JSON objects
    // are newline-delimited. Return the last one.
    let lines: Vec<&str> = body
        .trim()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    let last_line = lines.last().unwrap_or(&"");

    let value: serde_json::Value = serde_json::from_str(last_line).map_err(|e| {
        format!(
            "JSON parse error: {e}, body: {}",
            &body[..body.len().min(200)]
        )
    })?;

    if let Some(err) = value.get("error").and_then(|v| v.as_str()) {
        return Err(format!("docker error: {err}"));
    }

    Ok(value)
}

fn docker_image_digest(image_inspect: &serde_json::Value) -> Option<String> {
    image_inspect
        .get("RepoDigests")
        .and_then(|v| v.as_array())
        .and_then(|digests| digests.iter().find_map(|v| v.as_str()))
        .or_else(|| image_inspect.get("Id").and_then(|v| v.as_str()))
        .map(str::to_string)
}

fn docker_image_id(image_inspect: &serde_json::Value) -> Option<String> {
    image_inspect
        .get("Id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn image_identity_changed(previous_image_id: &str, new_image_id: &str) -> bool {
    previous_image_id != "unknown" && new_image_id != "unknown" && previous_image_id != new_image_id
}

fn docker_api_escape(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('/', "%2F")
        .replace(':', "%3A")
        .replace('@', "%40")
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
    fn test_proxy_filter_param_deserialize() {
        let json = r#"{"protocol":"socks5"}"#;
        let param: ProxyFilterParam = serde_json::from_str(json).unwrap();
        assert_eq!(param.protocol.as_deref(), Some("socks5"));
        assert!(param.country.is_none());
    }

    #[test]
    fn test_proxy_filter_param_optional() {
        let json = r#"{}"#;
        let param: ProxyFilterParam = serde_json::from_str(json).unwrap();
        assert!(param.protocol.is_none());
    }

    #[test]
    fn test_proxy_filter_param_with_filters() {
        let json = r#"{"protocol":"http","country":"US","anonymity":"elite","max_latency":500.0,"alive":true}"#;
        let param: ProxyFilterParam = serde_json::from_str(json).unwrap();
        assert_eq!(param.protocol.as_deref(), Some("http"));
        assert_eq!(param.country.as_deref(), Some("US"));
        assert_eq!(param.anonymity.as_deref(), Some("elite"));
        assert_eq!(param.max_latency, Some(500.0));
        assert_eq!(param.alive, Some(true));
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
    fn test_refresh_fetcher_param_deserialize() {
        let json = r#"{"fetcher":"proxyscrape:http"}"#;
        let param: RefreshFetcherParam = serde_json::from_str(json).unwrap();
        assert_eq!(param.fetcher, "proxyscrape:http");
    }

    #[test]
    fn test_route_test_param_deserialize() {
        let json = r#"{"host":"github.com","protocol":"socks5"}"#;
        let param: RouteTestParam = serde_json::from_str(json).unwrap();
        assert_eq!(param.host, "github.com");
        assert_eq!(param.protocol.as_deref(), Some("socks5"));
    }

    #[test]
    fn test_route_test_param_optional_protocol() {
        let json = r#"{"host":"github.com"}"#;
        let param: RouteTestParam = serde_json::from_str(json).unwrap();
        assert_eq!(param.host, "github.com");
        assert!(param.protocol.is_none());
    }

    #[test]
    fn test_cleanup_low_score_param_deserialize() {
        let json = r#"{"protocol":"http","limit":50,"min_score":0.25,"apply":true}"#;
        let param: CleanupLowScoreParam = serde_json::from_str(json).unwrap();
        assert_eq!(param.protocol.as_deref(), Some("http"));
        assert_eq!(param.limit, Some(50));
        assert_eq!(param.min_score, Some(0.25));
        assert_eq!(param.apply, Some(true));
    }

    #[test]
    fn test_cleanup_low_score_param_defaults_optional() {
        let json = r#"{}"#;
        let param: CleanupLowScoreParam = serde_json::from_str(json).unwrap();
        assert!(param.protocol.is_none());
        assert!(param.limit.is_none());
        assert!(param.min_score.is_none());
        assert!(param.apply.is_none());
    }

    #[test]
    fn test_scheduler_handle_clone() {
        let (cmd_tx, _cmd_rx) = mpsc::channel::<SchedulerCommand>(8);
        let handle = SchedulerHandle::new(cmd_tx);
        let _handle2 = handle.clone();
    }

    #[test]
    fn test_update_service_config_defaults_to_disabled() {
        let config = UpdateServiceConfig::from_lookup(|_| None);
        assert!(!config.enabled);
        assert_eq!(config.socket_path, "/var/run/docker.sock");
        assert_eq!(config.container_name, "proxy-pool");
        assert_eq!(config.image, "ghcr.io/iamdreaming/proxy-pool-rust:latest");
        assert_eq!(config.image_repo, "ghcr.io/iamdreaming/proxy-pool-rust");
        assert_eq!(config.image_tag, "latest");
        assert_eq!(
            config.watchtower_url,
            "http://watchtower-proxy-pool:8080/v1/update"
        );
        assert_eq!(config.watchtower_token, None);
    }

    #[test]
    fn test_update_service_config_from_lookup() {
        let config = UpdateServiceConfig::from_lookup(|key| match key {
            "PROXY_POOL_UPDATE_ENABLED" => Some("true".into()),
            "PROXY_POOL_UPDATE_DOCKER_SOCKET" => Some("/tmp/docker.sock".into()),
            "PROXY_POOL_UPDATE_CONTAINER" => Some("proxy-pool-blue".into()),
            "PROXY_POOL_UPDATE_IMAGE" => Some("localhost:5000/proxy-pool:test".into()),
            "PROXY_POOL_UPDATE_WATCHTOWER_URL" => Some("http://watchtower/v1/update".into()),
            "PROXY_POOL_UPDATE_TOKEN" => Some("secret".into()),
            _ => None,
        });

        assert!(config.enabled);
        assert_eq!(config.socket_path, "/tmp/docker.sock");
        assert_eq!(config.container_name, "proxy-pool-blue");
        assert_eq!(config.image, "localhost:5000/proxy-pool:test");
        assert_eq!(config.image_repo, "localhost:5000/proxy-pool");
        assert_eq!(config.image_tag, "test");
        assert_eq!(config.watchtower_url, "http://watchtower/v1/update");
        assert_eq!(config.watchtower_token.as_deref(), Some("secret"));
    }

    #[test]
    fn test_parse_bool_env_truthy_values() {
        for value in ["1", "true", "TRUE", "yes", "on"] {
            assert!(parse_bool_env(Some(value)));
        }
        for value in [None, Some(""), Some("false"), Some("0"), Some("off")] {
            assert!(!parse_bool_env(value));
        }
    }

    #[test]
    fn test_split_image_ref_handles_registry_port_and_missing_tag() {
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
    fn test_docker_image_digest_prefers_repo_digest() {
        let image = serde_json::json!({
            "Id": "sha256:image-id",
            "RepoDigests": ["ghcr.io/iamdreaming/proxy-pool-rust@sha256:repo-digest"]
        });

        assert_eq!(
            docker_image_digest(&image).as_deref(),
            Some("ghcr.io/iamdreaming/proxy-pool-rust@sha256:repo-digest")
        );
    }

    #[test]
    fn test_docker_image_digest_falls_back_to_id() {
        let image = serde_json::json!({
            "Id": "sha256:image-id",
            "RepoDigests": []
        });

        assert_eq!(
            docker_image_digest(&image).as_deref(),
            Some("sha256:image-id")
        );
    }

    #[test]
    fn test_docker_image_id_and_identity_change() {
        let image = serde_json::json!({
            "Id": "sha256:new-image-id",
            "RepoDigests": ["ghcr.io/iamdreaming/proxy-pool-rust@sha256:repo-digest"]
        });

        assert_eq!(
            docker_image_id(&image).as_deref(),
            Some("sha256:new-image-id")
        );
        assert!(image_identity_changed(
            "sha256:old-image-id",
            "sha256:new-image-id"
        ));
        assert!(!image_identity_changed(
            "sha256:new-image-id",
            "sha256:new-image-id"
        ));
        assert!(!image_identity_changed("unknown", "sha256:new-image-id"));
    }
}
