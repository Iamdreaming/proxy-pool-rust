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

pub mod rest_client;
pub mod serve;

use proxy_core::geoip::GeoIPLookup;
use proxy_core::models::ProxyFilter;
use proxy_core::status::{parse_bool_env, split_image_ref};
use proxy_core::store::ProxyStore;
use proxy_core::validator::{ProxyCheckMatrixRequest, ProxyCheckMatrixTarget, check_proxy_matrix};
use rest_client::{RestClient, urlencode};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::tool_handler;
use rmcp::{ServerHandler, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};

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
pub struct CheckProxyMatrixParam {
    pub host: String,
    pub port: u16,
    /// Proxy protocol: http, https, socks4, socks5.
    pub protocol: String,
    /// Optional validation targets. Defaults to Cloudflare trace and httpbin IP.
    pub targets: Option<Vec<CheckProxyMatrixTargetParam>>,
    /// Optional per-target timeout in seconds. Defaults to 10.
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(untagged)]
pub enum CheckProxyMatrixTargetParam {
    Url(String),
    Structured(CheckProxyMatrixStructuredTargetParam),
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CheckProxyMatrixStructuredTargetParam {
    pub url: String,
    #[serde(default)]
    pub expected_statuses: Vec<u16>,
}

impl From<CheckProxyMatrixTargetParam> for ProxyCheckMatrixTarget {
    fn from(target: CheckProxyMatrixTargetParam) -> Self {
        match target {
            CheckProxyMatrixTargetParam::Url(url) => Self::Url(url),
            CheckProxyMatrixTargetParam::Structured(target) => Self::Structured {
                url: target.url,
                expected_statuses: target.expected_statuses,
            },
        }
    }
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
pub struct RefreshSubscriptionSourceParam {
    /// Stable subscription source id, such as "static-url-1" or "aggregator-1".
    pub source: String,
    /// Apply writes to the pool and pending encrypted store. Defaults to false.
    pub apply: Option<bool>,
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ContainerLogsParam {
    /// Container name or id. Defaults to the configured update target
    /// (PROXY_POOL_UPDATE_CONTAINER, i.e. the main proxy-pool service).
    pub container: Option<String>,
    /// Number of trailing log lines to return. Defaults to 200, clamped to 1..=1000.
    pub tail: Option<u32>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum UpdateStatusKind {
    NeverTriggered,
    Disabled,
    InProgress,
    AlreadyCurrent,
    Updated,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct UpdateStatusSnapshot {
    #[serde(rename = "status")]
    status: UpdateStatusKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recorded_at_unix_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    update_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    container_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    watchtower_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_image_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_image_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    digest_changed: Option<bool>,
}

impl UpdateStatusSnapshot {
    fn never_triggered() -> Self {
        Self {
            status: UpdateStatusKind::NeverTriggered,
            message: None,
            recorded_at_unix_secs: None,
            update_enabled: None,
            container_name: None,
            image: None,
            image_repo: None,
            image_tag: None,
            watchtower_url: None,
            previous_image_id: None,
            new_image_id: None,
            new_digest: None,
            digest_changed: None,
        }
    }

    fn disabled(config: &UpdateServiceConfig) -> Self {
        Self::from_config(
            UpdateStatusKind::Disabled,
            config,
            Some(
                "update_service is disabled; set PROXY_POOL_UPDATE_ENABLED=true to allow updates"
                    .into(),
            ),
        )
    }

    fn in_progress(config: &UpdateServiceConfig) -> Self {
        Self::from_config(
            UpdateStatusKind::InProgress,
            config,
            Some("update in progress; poll update_status for the result".into()),
        )
    }

    fn failed(config: &UpdateServiceConfig, message: impl Into<String>) -> Self {
        Self::from_config(UpdateStatusKind::Failed, config, Some(message.into()))
    }

    fn already_current(config: &UpdateServiceConfig) -> Self {
        Self::from_config(UpdateStatusKind::AlreadyCurrent, config, None)
    }

    fn updated(config: &UpdateServiceConfig) -> Self {
        Self::from_config(UpdateStatusKind::Updated, config, None)
    }

    fn from_config(
        status: UpdateStatusKind,
        config: &UpdateServiceConfig,
        message: Option<String>,
    ) -> Self {
        Self {
            status,
            message,
            recorded_at_unix_secs: now_unix_secs(),
            update_enabled: Some(config.enabled),
            container_name: Some(config.container_name.clone()),
            image: Some(config.image.clone()),
            image_repo: Some(config.image_repo.clone()),
            image_tag: Some(config.image_tag.clone()),
            watchtower_url: Some(config.watchtower_url.clone()),
            previous_image_id: None,
            new_image_id: None,
            new_digest: None,
            digest_changed: None,
        }
    }

    fn with_previous_image_id(mut self, previous_image_id: impl Into<String>) -> Self {
        self.previous_image_id = Some(previous_image_id.into());
        self
    }

    fn with_image_result(
        mut self,
        previous_image_id: impl Into<String>,
        new_image_id: impl Into<String>,
        new_digest: impl Into<String>,
        digest_changed: bool,
    ) -> Self {
        self.previous_image_id = Some(previous_image_id.into());
        self.new_image_id = Some(new_image_id.into());
        self.new_digest = Some(new_digest.into());
        self.digest_changed = Some(digest_changed);
        self
    }
}

fn now_unix_secs() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

// ---------------------------------------------------------------------------
// MCP Server implementation
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ProxyPoolMcp {
    store: Arc<ProxyStore>,
    geoip: Option<Arc<Mutex<GeoIPLookup>>>,
    /// Client for the main service REST API (in-proc-only tools proxy here).
    rest: RestClient,
    update_status: Arc<RwLock<UpdateStatusSnapshot>>,
    tool_router: ToolRouter<Self>,
}

/// Dependencies required to construct the standalone MCP service.
pub struct ProxyPoolMcpConfig {
    /// Redis-backed pool store (shared with the main service via Redis).
    pub store: Arc<ProxyStore>,
    /// Optional local GeoIP lookup (MMDB file + Redis cache).
    pub geoip: Option<Arc<Mutex<GeoIPLookup>>>,
    /// Base URL of the main service REST API, e.g. `http://proxy-pool:8000`.
    pub upstream_api_url: String,
}

impl ProxyPoolMcp {
    pub fn new(config: ProxyPoolMcpConfig) -> Self {
        Self {
            store: config.store,
            geoip: config.geoip,
            rest: RestClient::new(config.upstream_api_url),
            update_status: Arc::new(RwLock::new(UpdateStatusSnapshot::never_triggered())),
            tool_router: Self::tool_router(),
        }
    }

    fn resolve_protocol(&self, protocol: Option<&str>) -> proxy_core::models::Protocol {
        protocol
            .and_then(proxy_core::models::Protocol::from_str_loose)
            .unwrap_or(proxy_core::models::Protocol::Http)
    }

    /// Format a REST call result as an MCP tool response, mapping errors to a
    /// structured `{"status":"error","message":...}` payload (never hang/panic).
    fn rest_result(result: Result<serde_json::Value, rest_client::RestError>) -> String {
        match result {
            Ok(value) => to_json(value),
            Err(e) => to_json(serde_json::json!({
                "status": "error",
                "message": e.to_string(),
            })),
        }
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

    async fn record_update_status(&self, status: UpdateStatusSnapshot) {
        *self.update_status.write().await = status;
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
        description = "Check if a specific proxy works against multiple validation targets. Protocol must be http, https, socks4, or socks5. Optional targets default to Cloudflare trace and httpbin IP."
    )]
    async fn check_proxy_matrix(&self, params: Parameters<CheckProxyMatrixParam>) -> String {
        let request = ProxyCheckMatrixRequest {
            host: params.0.host,
            port: params.0.port,
            protocol: params.0.protocol,
            targets: params.0.targets.map(|targets| {
                targets
                    .into_iter()
                    .map(ProxyCheckMatrixTarget::from)
                    .collect()
            }),
            timeout_secs: params.0.timeout_secs,
        };

        match check_proxy_matrix(request).await {
            Ok(result) => to_json(serde_json::to_value(result).unwrap_or_default()),
            Err(e) => to_json(serde_json::json!({
                "status": "error",
                "message": e.to_string(),
            })),
        }
    }

    #[tool(
        description = "Get structured service status, including version, uptime, Redis, pool, WARP, and xray summaries"
    )]
    async fn service_status(&self) -> String {
        Self::rest_result(self.rest.get_json("/api/status", &[]).await)
    }

    #[tool(description = "Get xray node lifecycle status and recent activation failures")]
    async fn xray_status(&self) -> String {
        Self::rest_result(self.rest.get_json("/api/xray/status", &[]).await)
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
        match self.rest.get_json("/api/warp", &[]).await {
            Ok(body) => {
                // /api/warp returns all instances with a `healthy` flag; preserve
                // this tool's historical healthy-only shape.
                let healthy: Vec<serde_json::Value> = body
                    .get("instances")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter(|inst| {
                                inst.get("healthy")
                                    .and_then(|h| h.as_bool())
                                    .unwrap_or(false)
                            })
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                to_json(serde_json::json!({
                    "warp": {
                        "healthy_count": healthy.len(),
                        "instances": healthy,
                    }
                }))
            }
            Err(e) => to_json(serde_json::json!({
                "status": "error",
                "message": e.to_string(),
            })),
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
        Self::rest_result(self.rest.post_json("/api/proxies/refresh", None).await)
    }

    #[tool(description = "Get the latest status report for each configured proxy fetcher")]
    async fn fetcher_status(&self) -> String {
        Self::rest_result(self.rest.get_json("/api/fetchers", &[]).await)
    }

    #[tool(description = "Get configured subscription source status and latest refresh reports")]
    async fn subscription_sources(&self) -> String {
        Self::rest_result(self.rest.get_json("/api/subscriptions/sources", &[]).await)
    }

    #[tool(
        description = "Preview or apply refresh for one configured subscription source by id. Defaults to preview with apply=false."
    )]
    async fn refresh_subscription_source(
        &self,
        params: Parameters<RefreshSubscriptionSourceParam>,
    ) -> String {
        let source = params.0.source;
        let apply = params.0.apply.unwrap_or(false);
        let path = format!("/api/subscriptions/sources/{}/refresh", urlencode(&source));
        Self::rest_result(
            self.rest
                .post_json_query(&path, &[("apply", apply.to_string())], None)
                .await,
        )
    }

    #[tool(
        description = "Refresh one configured proxy fetcher by id, such as proxyscrape:http or geonode"
    )]
    async fn refresh_fetcher(&self, params: Parameters<RefreshFetcherParam>) -> String {
        let path = format!("/api/fetchers/{}/refresh", urlencode(&params.0.fetcher));
        Self::rest_result(self.rest.post_json(&path, None).await)
    }

    #[tool(
        description = "Test gateway route selection for a host. Optionally specify protocol: http, https, socks4, socks5"
    )]
    async fn route_test(&self, params: Parameters<RouteTestParam>) -> String {
        let protocol = self
            .resolve_protocol(params.0.protocol.as_deref())
            .to_string();
        Self::rest_result(
            self.rest
                .get_json(
                    "/api/routes/test",
                    &[("host", params.0.host), ("protocol", protocol)],
                )
                .await,
        )
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

    #[tool(description = "Get the latest update_service result without triggering an update")]
    async fn update_status(&self) -> String {
        let status = self.update_status.read().await.clone();
        to_json(serde_json::to_value(status).unwrap_or_default())
    }

    #[tool(
        description = "Update the proxy-pool service by pulling the configured Docker image and triggering Watchtower. Requires PROXY_POOL_UPDATE_ENABLED=true."
    )]
    async fn update_service(&self) -> String {
        let config = UpdateServiceConfig::from_env();
        if !config.enabled {
            self.record_update_status(UpdateStatusSnapshot::disabled(&config))
                .await;
            return to_json(serde_json::json!({
                "status": "disabled",
                "message": "update_service is disabled; set PROXY_POOL_UPDATE_ENABLED=true to allow Docker/Watchtower updates",
                "required_env": "PROXY_POOL_UPDATE_ENABLED=true",
                "image": config.image,
                "container_name": config.container_name,
            }));
        }

        if config.watchtower_token.is_none() {
            self.record_update_status(UpdateStatusSnapshot::failed(
                &config,
                "PROXY_POOL_UPDATE_TOKEN must be set when update_service is enabled",
            ))
            .await;
            return to_json(serde_json::json!({
                "status": "error",
                "message": "PROXY_POOL_UPDATE_TOKEN must be set when update_service is enabled",
            }));
        }

        // Run the pull + Watchtower trigger in the background so the MCP call
        // returns immediately; the client polls `update_status` for the result.
        // The MCP process is separate from proxy-pool now, so this task survives
        // the container recreation and records the terminal snapshot.
        self.record_update_status(UpdateStatusSnapshot::in_progress(&config))
            .await;
        let update_status = self.update_status.clone();
        let cfg = config.clone();
        tokio::spawn(async move {
            let timeout = std::time::Duration::from_secs(UPDATE_SERVICE_TIMEOUT_SECS);
            if tokio::time::timeout(timeout, run_update(update_status.clone(), cfg.clone()))
                .await
                .is_err()
            {
                *update_status.write().await = UpdateStatusSnapshot::failed(
                    &cfg,
                    format!("update timed out after {UPDATE_SERVICE_TIMEOUT_SECS}s"),
                );
            }
        });

        to_json(serde_json::json!({
            "status": "update_started",
            "message": "Update started in the background. Poll update_status for progress and the result.",
            "image": config.image,
            "container_name": config.container_name,
        }))
    }

    #[tool(
        description = "Fetch recent logs from a Docker container (default: the proxy-pool service). Params: container (name/id, optional), tail (line count, default 200, max 1000)."
    )]
    async fn container_logs(&self, params: Parameters<ContainerLogsParam>) -> String {
        let config = UpdateServiceConfig::from_env();
        let container = params
            .0
            .container
            .filter(|c| !c.trim().is_empty())
            .unwrap_or(config.container_name);
        let tail = params.0.tail.unwrap_or(200).clamp(1, 1000);
        let path = format!(
            "/containers/{}/logs?stdout=1&stderr=1&timestamps=1&tail={}",
            docker_api_escape(&container),
            tail
        );
        match docker_api_get_raw(&config.socket_path, &path).await {
            Ok(body) => to_json(serde_json::json!({
                "status": "ok",
                "container": container,
                "tail": tail,
                "logs": demux_docker_log_stream(&body),
            })),
            Err(e) => to_json(serde_json::json!({
                "status": "error",
                "container": container,
                "message": e,
            })),
        }
    }
}

/// Overall safety timeout for a background `update_service` run.
const UPDATE_SERVICE_TIMEOUT_SECS: u64 = 300;

/// Max attempts for the image pull (retries transient registry timeouts).
const PULL_MAX_ATTEMPTS: usize = 3;

/// Base backoff between pull attempts (multiplied by the attempt number).
const PULL_RETRY_BACKOFF_SECS: u64 = 3;

/// Background body of `update_service`: inspect → pull → compare → trigger
/// Watchtower, writing the terminal snapshot into `update_status`.
async fn run_update(update_status: Arc<RwLock<UpdateStatusSnapshot>>, config: UpdateServiceConfig) {
    async fn set(status: &Arc<RwLock<UpdateStatusSnapshot>>, snapshot: UpdateStatusSnapshot) {
        *status.write().await = snapshot;
    }

    let Some(watchtower_token) = config.watchtower_token.as_deref() else {
        set(
            &update_status,
            UpdateStatusSnapshot::failed(&config, "PROXY_POOL_UPDATE_TOKEN missing"),
        )
        .await;
        return;
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
            set(
                &update_status,
                UpdateStatusSnapshot::failed(&config, format!("failed to inspect container: {e}")),
            )
            .await;
            return;
        }
    };

    let previous_image_id = old_inspect
        .get("Image")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    // Step 2: Pull latest image (pre-fetch so Watchtower doesn't need to).
    //
    // Retry on transient failures: the daemon's anonymous token fetch to
    // ghcr.io can time out on a flaky network, whereas an interactive
    // `docker compose pull` succeeds thanks to cached tokens / CLI retries.
    let pull_path = format!(
        "/images/create?fromImage={}&tag={}",
        docker_api_escape(&config.image_repo),
        docker_api_escape(&config.image_tag)
    );
    let mut pull_error = None;
    for attempt in 1..=PULL_MAX_ATTEMPTS {
        tracing::info!(
            image = %config.image,
            attempt,
            "update_service: pulling image"
        );
        match docker_api_post(&config.socket_path, &pull_path).await {
            Ok(_) => {
                pull_error = None;
                break;
            }
            Err(e) => {
                tracing::warn!(
                    "update_service: docker pull attempt {attempt}/{PULL_MAX_ATTEMPTS} failed: {e}"
                );
                pull_error = Some(e);
                if attempt < PULL_MAX_ATTEMPTS {
                    tokio::time::sleep(std::time::Duration::from_secs(
                        PULL_RETRY_BACKOFF_SECS * attempt as u64,
                    ))
                    .await;
                }
            }
        }
    }
    if let Some(e) = pull_error {
        set(
            &update_status,
            UpdateStatusSnapshot::failed(
                &config,
                format!("docker pull failed after {PULL_MAX_ATTEMPTS} attempts: {e}"),
            )
            .with_previous_image_id(previous_image_id.clone()),
        )
        .await;
        return;
    }

    let new_inspect = match docker_api_get(
        &config.socket_path,
        &format!("/images/{}/json", docker_api_escape(&config.image)),
    )
    .await
    {
        Ok(body) => body,
        Err(e) => {
            set(
                &update_status,
                UpdateStatusSnapshot::failed(
                    &config,
                    format!("failed to inspect pulled image: {e}"),
                )
                .with_previous_image_id(previous_image_id.clone()),
            )
            .await;
            return;
        }
    };
    let new_digest = docker_image_digest(&new_inspect).unwrap_or_else(|| "unknown".into());
    let new_image_id = docker_image_id(&new_inspect).unwrap_or_else(|| "unknown".into());
    let digest_changed = image_identity_changed(&previous_image_id, &new_image_id);

    if !digest_changed {
        set(
            &update_status,
            UpdateStatusSnapshot::already_current(&config).with_image_result(
                previous_image_id,
                new_image_id,
                new_digest,
                false,
            ),
        )
        .await;
        return;
    }

    // Step 3: Trigger Watchtower to recreate the container.
    tracing::info!("update_service: triggering Watchtower update");
    let client = reqwest::Client::new();
    let resp = client
        .post(&config.watchtower_url)
        .header("Authorization", format!("Bearer {watchtower_token}"))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await;

    let snapshot = match resp {
        Ok(r) if r.status().is_success() => {
            tracing::info!("update_service: Watchtower update triggered successfully");
            UpdateStatusSnapshot::updated(&config)
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            UpdateStatusSnapshot::failed(&config, format!("Watchtower returned {status}: {body}"))
        }
        Err(e) => UpdateStatusSnapshot::failed(&config, format!("failed to reach Watchtower: {e}")),
    };
    set(
        &update_status,
        snapshot.with_image_result(previous_image_id, new_image_id, new_digest, digest_changed),
    )
    .await;
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

/// GET a Docker Engine API path and return the raw (non-JSON) response body.
///
/// Sends `Connection: close` so the daemon closes the socket after the body,
/// letting us read to EOF safely (used for the container-logs stream, which is
/// binary-framed and must not be parsed as JSON). Dechunks if needed.
#[cfg(unix)]
async fn docker_api_get_raw(socket_path: &str, path: &str) -> Result<Vec<u8>, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;
    use tokio::time::{Duration, timeout};

    let mut stream = timeout(Duration::from_secs(10), UnixStream::connect(socket_path))
        .await
        .map_err(|_| format!("connect to {socket_path}: timed out after 10s"))?
        .map_err(|e| format!("connect to {socket_path}: {e}"))?;

    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    timeout(
        Duration::from_secs(10),
        stream.write_all(request.as_bytes()),
    )
    .await
    .map_err(|_| "write: timed out after 10s".to_string())?
    .map_err(|e| format!("write: {e}"))?;

    let mut buf = Vec::with_capacity(16384);
    let mut tmp = [0u8; 16384];
    let max_size = 16_777_216; // 16 MiB cap
    loop {
        match timeout(Duration::from_secs(15), stream.read(&mut tmp)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.len() > max_size {
                    break;
                }
            }
            Ok(Err(e)) => return Err(format!("read: {e}")),
            Err(_) => return Err("read: timed out after 15s".into()),
        }
    }

    let sep = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no HTTP header terminator in response")?;
    let header_bytes = &buf[..sep];
    let header = String::from_utf8_lossy(header_bytes);
    let status_line = header.lines().next().unwrap_or("");
    if !(status_line.contains(" 200") || status_line.contains(" 101")) {
        return Err(format!("docker returned non-200: {status_line}"));
    }
    let header_lower = header.to_ascii_lowercase();
    let body = &buf[sep + 4..];
    if header_lower.contains("transfer-encoding: chunked") {
        Ok(dechunk_bytes(body))
    } else {
        Ok(body.to_vec())
    }
}

#[cfg(not(unix))]
async fn docker_api_get_raw(_socket_path: &str, _path: &str) -> Result<Vec<u8>, String> {
    Err("Docker Engine API is only available on Unix (requires Unix socket)".into())
}

/// Decode an HTTP chunked-transfer body into raw bytes (byte-accurate; safe for
/// binary payloads).
#[cfg(unix)]
fn dechunk_bytes(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        // Read the chunk-size line up to CRLF.
        let Some(nl) = body[i..].windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let size_str = String::from_utf8_lossy(&body[i..i + nl]);
        let size = match usize::from_str_radix(size_str.trim(), 16) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        let data_start = i + nl + 2;
        let data_end = (data_start + size).min(body.len());
        out.extend_from_slice(&body[data_start..data_end]);
        i = data_end + 2; // skip trailing CRLF
    }
    out
}

/// Demultiplex Docker's non-TTY log stream (8-byte frame header + payload) into
/// plain text, merging stdout/stderr in arrival order.
///
/// Falls back to a lossy UTF-8 decode of the whole buffer when the data is not
/// framed (TTY containers emit raw bytes).
fn demux_docker_log_stream(body: &[u8]) -> String {
    // A valid frame header is [stream(1..=2), 0, 0, 0, size(4 BE)].
    let looks_framed = body.len() >= 8 && matches!(body[0], 1 | 2) && body[1..4] == [0, 0, 0];
    if !looks_framed {
        return String::from_utf8_lossy(body).into_owned();
    }
    let mut out: Vec<u8> = Vec::with_capacity(body.len());
    let mut i = 0;
    while i + 8 <= body.len() {
        let stream = body[i];
        if !matches!(stream, 1 | 2) || body[i + 1..i + 4] != [0, 0, 0] {
            // Framing broke — append the remainder raw and stop.
            out.extend_from_slice(&body[i..]);
            break;
        }
        let size =
            u32::from_be_bytes([body[i + 4], body[i + 5], body[i + 6], body[i + 7]]) as usize;
        let start = i + 8;
        let end = (start + size).min(body.len());
        out.extend_from_slice(&body[start..end]);
        i = end;
    }
    String::from_utf8_lossy(&out).into_owned()
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
    fn test_check_proxy_matrix_param_deserialize() {
        let json = r#"{
            "host":"1.2.3.4",
            "port":8080,
            "protocol":"socks5",
            "targets":[
                "https://example.com",
                {
                    "url":"https://api.openai.com/v1/models",
                    "expected_statuses":[401]
                }
            ],
            "timeout_secs":3
        }"#;
        let param: CheckProxyMatrixParam = serde_json::from_str(json).unwrap();
        assert_eq!(param.host, "1.2.3.4");
        assert_eq!(param.port, 8080);
        assert_eq!(param.protocol, "socks5");
        assert_eq!(
            param.targets.as_deref(),
            Some(
                [
                    CheckProxyMatrixTargetParam::Url("https://example.com".to_string()),
                    CheckProxyMatrixTargetParam::Structured(
                        CheckProxyMatrixStructuredTargetParam {
                            url: "https://api.openai.com/v1/models".into(),
                            expected_statuses: vec![401],
                        }
                    ),
                ]
                .as_slice()
            )
        );
        assert_eq!(param.timeout_secs, Some(3));
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
    fn test_refresh_subscription_source_param_deserialize() {
        let json = r#"{"source":"static-url-1","apply":true}"#;
        let param: RefreshSubscriptionSourceParam = serde_json::from_str(json).unwrap();
        assert_eq!(param.source, "static-url-1");
        assert_eq!(param.apply, Some(true));

        let preview_json = r#"{"source":"aggregator-1"}"#;
        let preview: RefreshSubscriptionSourceParam = serde_json::from_str(preview_json).unwrap();
        assert_eq!(preview.source, "aggregator-1");
        assert_eq!(preview.apply, None);
    }

    #[test]
    fn test_subscription_report_serializes_recommendation_for_tools() {
        let report: proxy_sub::ops::SubscriptionSourceReport =
            serde_json::from_value(serde_json::json!({
                "source": {
                    "id": "static-url-1",
                    "kind": "static_url",
                    "label": "https://example.com/sub",
                    "enabled": true
                },
                "mode": "preview",
                "started_at": "2026-07-07T00:00:00Z",
                "finished_at": "2026-07-07T00:00:01Z",
                "elapsed_ms": 10,
                "outcome": "ok",
                "last_error": null,
                "discovered_urls": 1,
                "unique_urls": 1,
                "duplicate_urls": 0,
                "fetched_urls": 1,
                "failed_urls": 0,
                "parsed_nodes": 20,
                "direct_nodes": 5,
                "encrypted_nodes": 15,
                "unknown_nodes": 0,
                "duplicate_nodes": 0,
                "stored_basic": 0,
                "stored_encrypted": 0,
                "protocol_counts": {},
                "errors": [],
                "recommendation": {
                    "decision": "apply",
                    "grade": 95,
                    "reasons": ["source_meets_apply_thresholds"],
                    "metrics": {
                        "fetch_success_rate": 1.0,
                        "supported_protocol_ratio": 1.0,
                        "unknown_node_ratio": 0.0,
                        "duplicate_node_ratio": 0.0,
                        "parsed_nodes_per_url": 20.0
                    }
                }
            }))
            .unwrap();

        let json = to_json(serde_json::json!({ "status": "ok", "report": report }));

        assert!(json.contains("\"recommendation\""));
        assert!(json.contains("\"decision\": \"apply\""));
        assert!(json.contains("\"grade\": 95"));
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
    fn test_xray_status_snapshot_serializes_tool_contract() {
        use proxy_core::xray_status::XrayStatusSnapshot;
        let snapshot = XrayStatusSnapshot {
            enabled: true,
            active_nodes: 3,
            activating_nodes: 0,
            failed_nodes: 1,
            removed_nodes: 0,
            total_nodes: 4,
            recent_nodes: vec![],
        };
        let json = to_json(serde_json::to_value(snapshot).unwrap_or_default());
        assert!(json.contains("\"active_nodes\": 3"));
        assert!(json.contains("\"failed_nodes\": 1"));
        assert!(json.contains("\"recent_nodes\": []"));
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
    fn test_container_logs_param_and_tail_clamp() {
        let param: ContainerLogsParam = serde_json::from_str(r#"{"tail":5000}"#).unwrap();
        assert_eq!(param.tail.unwrap_or(200).clamp(1, 1000), 1000);
        let param: ContainerLogsParam = serde_json::from_str(r#"{}"#).unwrap();
        assert!(param.container.is_none());
        assert_eq!(param.tail.unwrap_or(200).clamp(1, 1000), 200);
        let param: ContainerLogsParam = serde_json::from_str(r#"{"tail":0}"#).unwrap();
        assert_eq!(param.tail.unwrap_or(200).clamp(1, 1000), 1);
    }

    #[test]
    fn test_demux_docker_log_stream_frames() {
        // Two frames: stdout "hello\n" then stderr "err\n".
        let mut buf = Vec::new();
        let payload1 = b"hello\n";
        buf.extend_from_slice(&[1, 0, 0, 0]);
        buf.extend_from_slice(&(payload1.len() as u32).to_be_bytes());
        buf.extend_from_slice(payload1);
        let payload2 = b"err\n";
        buf.extend_from_slice(&[2, 0, 0, 0]);
        buf.extend_from_slice(&(payload2.len() as u32).to_be_bytes());
        buf.extend_from_slice(payload2);

        assert_eq!(demux_docker_log_stream(&buf), "hello\nerr\n");
    }

    #[test]
    fn test_demux_docker_log_stream_raw_fallback() {
        // Not framed (TTY / plain text) → lossy passthrough.
        let raw = b"plain log line\n";
        assert_eq!(demux_docker_log_stream(raw), "plain log line\n");
    }

    #[test]
    fn test_demux_docker_log_stream_truncated_and_malformed() {
        // Empty buffer → empty string (no panic).
        assert_eq!(demux_docker_log_stream(&[]), "");

        // Size field larger than the remaining payload → copy what is present,
        // no out-of-bounds slice.
        let mut buf = vec![1, 0, 0, 0];
        buf.extend_from_slice(&100u32.to_be_bytes()); // claims 100 bytes
        buf.extend_from_slice(b"short"); // only 5 present
        assert_eq!(demux_docker_log_stream(&buf), "short");

        // A valid frame followed by a partial (<8 byte) trailing header → the
        // parsed frame is kept and the partial header is dropped, no panic.
        let mut buf = vec![1, 0, 0, 0];
        buf.extend_from_slice(&6u32.to_be_bytes());
        buf.extend_from_slice(b"hello\n");
        buf.extend_from_slice(&[2, 0, 0]); // 3-byte partial header
        assert_eq!(demux_docker_log_stream(&buf), "hello\n");

        // Framing breaks mid-stream (invalid stream byte) → keep the decoded
        // prefix, append the remainder raw, no panic.
        let mut buf = vec![1, 0, 0, 0];
        buf.extend_from_slice(&3u32.to_be_bytes());
        buf.extend_from_slice(b"abc");
        buf.extend_from_slice(&[9, 0, 0, 0, 0, 0, 0, 2, b'x', b'y']); // stream byte 9 invalid
        assert!(demux_docker_log_stream(&buf).starts_with("abc"));
    }

    #[test]
    fn test_update_status_in_progress_snapshot() {
        let config = UpdateServiceConfig::from_env();
        let snap = UpdateStatusSnapshot::in_progress(&config);
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"status\":\"in_progress\""));
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
    fn test_scored_proxy_json_includes_quality_trend() {
        let mut proxy =
            proxy_core::models::Proxy::new("1.2.3.4", 8080, proxy_core::models::Protocol::Http);
        proxy
            .quality_history
            .samples
            .push(proxy_core::models::QualitySample {
                checked_at_unix_secs: 1,
                success: true,
                latency_ms: Some(90.0),
                error: None,
            });
        let score = proxy_core::store::explain_score(
            &proxy,
            &proxy_core::config::ScoreWeights {
                latency: 0.5,
                success: 0.3,
                anonymity: 0.2,
            },
            0.1,
        );
        let scored = proxy_core::store::ScoredProxy { proxy, score };

        let json = to_json(serde_json::json!({
            "count": 1,
            "proxies": [scored],
        }));

        assert!(json.contains("\"trend\""));
        assert!(json.contains("\"recent_samples\": 1"));
        assert!(json.contains("\"recent_success_rate\": 1.0"));
        assert!(json.contains("\"recent_latency_p50\": 90.0"));
    }

    #[test]
    fn test_fetcher_status_json_includes_quality_fields() {
        let report = proxy_core::fetcher::base::FetcherRunReport {
            id: "proxyscrape:http".into(),
            name: "ProxyScrape".into(),
            status: proxy_core::fetcher::base::FetcherRunStatus::Success,
            fetched: 5,
            parsed: 4,
            unique: 4,
            validated: 2,
            stored: 1,
            validation_survival_rate: Some(0.5),
            error: None,
            circuit_state: proxy_core::fetcher::base::FetcherCircuitState::Closed,
            consecutive_failures: 0,
            last_error: None,
            last_attempt_at: None,
            last_success_at: None,
            opened_at: None,
            next_probe_at: None,
            action: Some(proxy_core::fetcher::base::FetcherRunAction::Fetched),
            started_at: None,
            finished_at: None,
            duration_ms: None,
        };

        let json = to_json(serde_json::json!({
            "fetchers": [report],
        }));

        assert!(json.contains("\"unique\": 4"));
        assert!(json.contains("\"validated\": 2"));
        assert!(json.contains("\"stored\": 1"));
        assert!(json.contains("\"validation_survival_rate\": 0.5"));
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
    fn test_update_status_default_serializes_never_triggered() {
        let status = UpdateStatusSnapshot::never_triggered();
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "{\"status\":\"never_triggered\"}");
    }

    #[test]
    fn test_update_status_disabled_carries_config() {
        let config = UpdateServiceConfig::from_lookup(|_| None);
        let status = UpdateStatusSnapshot::disabled(&config);
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"status\":\"disabled\""));
        assert!(json.contains("\"update_enabled\":false"));
        assert!(json.contains("\"container_name\":\"proxy-pool\""));
        assert!(json.contains("\"image\":\"ghcr.io/iamdreaming/proxy-pool-rust:latest\""));
        assert!(json.contains("\"recorded_at_unix_secs\""));
    }

    #[test]
    fn test_update_status_records_failures_and_image_result() {
        let config = UpdateServiceConfig::from_lookup(|key| match key {
            "PROXY_POOL_UPDATE_ENABLED" => Some("true".into()),
            "PROXY_POOL_UPDATE_IMAGE" => Some("localhost:5000/proxy-pool:test".into()),
            _ => None,
        });

        let failed = UpdateStatusSnapshot::failed(&config, "watchtower unavailable")
            .with_image_result("sha256:old", "sha256:new", "repo@sha256:digest", true);
        let json = serde_json::to_string(&failed).unwrap();
        assert!(json.contains("\"status\":\"failed\""));
        assert!(json.contains("watchtower unavailable"));
        assert!(json.contains("\"previous_image_id\":\"sha256:old\""));
        assert!(json.contains("\"new_image_id\":\"sha256:new\""));
        assert!(json.contains("\"digest_changed\":true"));
    }

    #[test]
    fn test_update_status_distinguishes_current_and_updated() {
        let config = UpdateServiceConfig::from_lookup(|_| None);
        let already_current = UpdateStatusSnapshot::already_current(&config).with_image_result(
            "sha256:same",
            "sha256:same",
            "repo@sha256:same",
            false,
        );
        let updated = UpdateStatusSnapshot::updated(&config).with_image_result(
            "sha256:old",
            "sha256:new",
            "repo@sha256:new",
            true,
        );

        let current_json = serde_json::to_string(&already_current).unwrap();
        let updated_json = serde_json::to_string(&updated).unwrap();
        assert!(current_json.contains("\"status\":\"already_current\""));
        assert!(current_json.contains("\"digest_changed\":false"));
        assert!(updated_json.contains("\"status\":\"updated\""));
        assert!(updated_json.contains("\"digest_changed\":true"));
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
