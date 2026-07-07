//! Configuration: YAML loading with defaults.

use serde::{Deserialize, Serialize};
use std::path::Path;

// ---------------------------------------------------------------------------
// Sub-configs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewaySettings {
    #[serde(default = "default_listen_host")]
    pub listen_host: String,
    #[serde(default = "default_gateway_port")]
    pub listen_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XraySettings {
    /// Whether xray integration is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Path to the xray-core binary.
    #[serde(default = "default_xray_binary")]
    pub binary_path: String,
    /// gRPC API port for xray-core's HandlerService.
    #[serde(default = "default_xray_api_port")]
    pub api_port: u16,
    /// Port range start for local SOCKS5 inbounds (inclusive).
    #[serde(default = "default_xray_port_start")]
    pub port_range_start: u16,
    /// Port range end for local SOCKS5 inbounds (inclusive).
    #[serde(default = "default_xray_port_end")]
    pub port_range_end: u16,
    /// Interval in seconds for the pending-to-active sync loop.
    #[serde(default = "default_xray_sync_interval")]
    pub sync_interval_sec: u64,
    /// Maximum number of active encrypted nodes.
    #[serde(default = "default_xray_max_nodes")]
    pub max_active_nodes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiSettings {
    #[serde(default = "default_listen_host")]
    pub listen_host: String,
    #[serde(default = "default_api_port")]
    pub listen_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSettings {
    /// Transport mode: "stdio", "http", or "both".
    #[serde(default = "default_mcp_transport")]
    pub transport: String,
    #[serde(default = "default_mcp_http_port")]
    pub http_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisSettings {
    #[serde(default = "default_redis_url")]
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreWeights {
    #[serde(default = "default_weight_latency")]
    pub latency: f64,
    #[serde(default = "default_weight_success")]
    pub success: f64,
    #[serde(default = "default_weight_anonymity")]
    pub anonymity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetcherToggle {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Whether to apply mirror_prefix to this fetcher's URLs.
    /// Set to false for fetchers whose source URLs are not compatible with the mirror.
    #[serde(default = "default_true")]
    pub use_mirror: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchersConfig {
    /// Optional URL prefix for mirroring GitHub raw URLs.
    /// When set, `raw.githubusercontent.com` URLs are prefixed with this value.
    /// Example: `https://v4.gh-proxy.org/`
    #[serde(default)]
    pub github_mirror_prefix: Option<String>,
    /// Optional URL prefix for mirroring non-GitHub URLs that may be blocked.
    /// When set, all non-GitHub fetcher URLs are prefixed with this value.
    /// Example: `https://v4.gh-proxy.org/`
    #[serde(default)]
    pub mirror_prefix: Option<String>,
    #[serde(default)]
    pub proxyscrape: FetcherToggle,
    #[serde(default)]
    pub thespeedx: FetcherToggle,
    #[serde(default)]
    pub free_proxy_list: FetcherToggle,
    #[serde(default)]
    pub clarketm: FetcherToggle,
    #[serde(default)]
    pub geonode: FetcherToggle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolSettings {
    #[serde(default = "default_fetch_interval")]
    pub fetch_interval_sec: u64,
    #[serde(default = "default_validate_interval")]
    pub validate_interval_sec: u64,
    #[serde(default = "default_validate_concurrency")]
    pub validate_concurrency: usize,
    #[serde(default = "default_validate_timeout")]
    pub validate_timeout_sec: u64,
    #[serde(default = "default_validate_target")]
    pub validate_target_url: String,
    #[serde(default)]
    pub validate_target_urls: Vec<String>,
    #[serde(default = "default_min_score")]
    pub min_score: f64,
    #[serde(default)]
    pub score_weights: ScoreWeights,
    #[serde(default)]
    pub fetchers: FetchersConfig,
    /// Max connection attempts per second (0 = unlimited).
    #[serde(default = "default_pace_rate")]
    pub pace_rate_per_sec: f64,
}

impl PoolSettings {
    /// Return validation targets with backward-compatible single-target fallback.
    pub fn effective_validate_target_urls(&self) -> Vec<String> {
        if self.validate_target_urls.is_empty() {
            return vec![self.validate_target_url.clone()];
        }
        self.validate_target_urls.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarpInstanceConfig {
    pub id: u32,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarpOptimizerConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_optimizer_interval")]
    pub interval_sec: u64,
    #[serde(default = "default_scan_ports")]
    pub scan_ports: Vec<u16>,
    #[serde(default = "default_max_loss_pct")]
    pub max_loss_pct: f64,
    #[serde(default = "default_scan_threads")]
    pub scan_threads: usize,
    #[serde(default = "default_assign")]
    pub assign: String,
    #[serde(default = "default_compose_file")]
    pub compose_file: String,
    #[serde(default = "default_scan_data_dir")]
    pub scan_data_dir: String,
    #[serde(default = "default_state_path")]
    pub state_path: String,
    #[serde(default = "default_health_timeout")]
    pub health_timeout_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarpSettings {
    #[serde(default)]
    pub instances: Vec<WarpInstanceConfig>,
    #[serde(default = "default_warp_health_interval")]
    pub health_check_interval_sec: u64,
    #[serde(default = "default_warp_health_timeout")]
    pub health_check_timeout_sec: u64,
    #[serde(default = "default_warp_health_url")]
    pub health_check_url: String,
    #[serde(default = "default_warp_fail_threshold")]
    pub health_check_fail_threshold: u32,
    #[serde(default)]
    pub optimizer: WarpOptimizerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoIpSettings {
    #[serde(default = "default_geoip_db_path")]
    pub database_path: String,
    #[serde(default = "default_geoip_cache_ttl")]
    pub cache_ttl_sec: u64,
    #[serde(default = "default_domestic_countries")]
    pub domestic_countries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreePoolSettings {
    #[serde(default = "default_max_retry")]
    pub max_retry: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubDiscoverConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default = "default_github_max")]
    pub max_results: u32,
    #[serde(default = "default_github_interval")]
    pub search_interval_sec: u64,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatorEntryConfig {
    pub url: String,
    #[serde(default = "default_agg_format")]
    pub format: String,
    #[serde(default = "default_agg_interval")]
    pub refresh_interval_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionConfig {
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub github: GitHubDiscoverConfig,
    #[serde(default)]
    pub aggregators: Vec<AggregatorEntryConfig>,
    #[serde(default = "default_sub_interval")]
    pub refresh_interval_sec: u64,
    #[serde(default = "default_sub_timeout")]
    pub fetch_timeout_sec: u64,
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_sec: u64,
}

// ---------------------------------------------------------------------------
// Top-level Settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub gateway: GatewaySettings,
    #[serde(default)]
    pub api: ApiSettings,
    #[serde(default)]
    pub mcp: McpSettings,
    #[serde(default)]
    pub redis: RedisSettings,
    #[serde(default)]
    pub pool: PoolSettings,
    #[serde(default)]
    pub warp: WarpSettings,
    #[serde(default)]
    pub geoip: GeoIpSettings,
    #[serde(default)]
    pub free_pool: FreePoolSettings,
    #[serde(default)]
    pub subscription: SubscriptionConfig,
    /// Path to the routing rules YAML file (optional).
    #[serde(default)]
    pub routes_path: Option<String>,
    #[serde(default)]
    pub xray: XraySettings,
}

impl Default for Settings {
    fn default() -> Self {
        // serde(default) on every field means an empty YAML produces valid
        // settings, but we still implement Default for programmatic use.
        serde_yaml::from_str("{}").unwrap()
    }
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Load settings from a YAML file. Missing keys fall back to defaults.
/// If the file does not exist, returns defaults.
pub fn load_settings(path: impl AsRef<Path>) -> Settings {
    let path = path.as_ref();
    if !path.exists() {
        return Settings::default();
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(
                "cannot read config file {}: {e}, using defaults",
                path.display()
            );
            return Settings::default();
        }
    };
    match serde_yaml::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(
                "invalid config file {}: {e}, using defaults",
                path.display()
            );
            Settings::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Default value functions
// ---------------------------------------------------------------------------

fn default_listen_host() -> String {
    "0.0.0.0".into()
}
fn default_gateway_port() -> u16 {
    9080
}
fn default_api_port() -> u16 {
    8000
}
fn default_mcp_transport() -> String {
    "both".into()
}
fn default_mcp_http_port() -> u16 {
    9000
}
fn default_redis_url() -> String {
    "redis://redis:6379/0".into()
}
fn default_weight_latency() -> f64 {
    0.5
}
fn default_weight_success() -> f64 {
    0.3
}
fn default_weight_anonymity() -> f64 {
    0.2
}
fn default_true() -> bool {
    true
}
fn default_fetch_interval() -> u64 {
    300
}
fn default_validate_interval() -> u64 {
    60
}
fn default_validate_concurrency() -> usize {
    50
}
fn default_validate_timeout() -> u64 {
    15
}
fn default_validate_target() -> String {
    "https://www.cloudflare.com/cdn-cgi/trace".into()
}
fn default_min_score() -> f64 {
    0.1
}
fn default_pace_rate() -> f64 {
    10.0
}
fn default_optimizer_interval() -> u64 {
    21600
}
fn default_scan_ports() -> Vec<u16> {
    vec![2408, 500, 1701, 4500]
}
fn default_max_loss_pct() -> f64 {
    10.0
}
fn default_scan_threads() -> usize {
    100
}
fn default_assign() -> String {
    "distinct".into()
}
fn default_compose_file() -> String {
    "deploy/warp/docker-compose.yml".into()
}
fn default_scan_data_dir() -> String {
    "deploy/warp/scan-data".into()
}
fn default_state_path() -> String {
    "deploy/warp/state.json".into()
}
fn default_health_timeout() -> u64 {
    30
}
fn default_warp_health_interval() -> u64 {
    30
}
fn default_warp_health_timeout() -> u64 {
    10
}
fn default_warp_health_url() -> String {
    "https://www.cloudflare.com/cdn-cgi/trace".into()
}
fn default_warp_fail_threshold() -> u32 {
    3
}
fn default_geoip_db_path() -> String {
    "/app/geoip/GeoLite2-Country.mmdb".into()
}
fn default_geoip_cache_ttl() -> u64 {
    86400
}
fn default_domestic_countries() -> Vec<String> {
    vec!["CN".into()]
}
fn default_max_retry() -> u32 {
    3
}
fn default_github_max() -> u32 {
    20
}
fn default_github_interval() -> u64 {
    86400
}
fn default_agg_format() -> String {
    "text".into()
}
fn default_agg_interval() -> u64 {
    43200
}
fn default_sub_interval() -> u64 {
    3600
}
fn default_sub_timeout() -> u64 {
    30
}
fn default_cache_ttl() -> u64 {
    1800
}
fn default_xray_binary() -> String {
    "xray".into()
}
fn default_xray_api_port() -> u16 {
    10085
}
fn default_xray_port_start() -> u16 {
    20000
}
fn default_xray_port_end() -> u16 {
    29999
}
fn default_xray_sync_interval() -> u64 {
    30
}
fn default_xray_max_nodes() -> usize {
    5000
}

// Default impls for sub-configs that need explicit Default
impl Default for GatewaySettings {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}
impl Default for ApiSettings {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}
impl Default for McpSettings {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}
impl Default for RedisSettings {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}
impl Default for ScoreWeights {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}
impl Default for FetcherToggle {
    fn default() -> Self {
        Self {
            enabled: true,
            use_mirror: true,
        }
    }
}
impl Default for FetchersConfig {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}
impl Default for PoolSettings {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}
impl Default for WarpOptimizerConfig {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}
impl Default for WarpSettings {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}
impl Default for GeoIpSettings {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}
impl Default for FreePoolSettings {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}
impl Default for GitHubDiscoverConfig {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}
impl Default for SubscriptionConfig {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}
impl Default for XraySettings {
    fn default() -> Self {
        serde_yaml::from_str("{}").unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_validate_target_urls_falls_back_to_single_target() {
        let settings = PoolSettings {
            validate_target_url: "https://one.example/check".into(),
            validate_target_urls: vec![],
            ..PoolSettings::default()
        };

        assert_eq!(
            settings.effective_validate_target_urls(),
            vec!["https://one.example/check".to_string()]
        );
    }

    #[test]
    fn effective_validate_target_urls_prefers_explicit_list() {
        let settings = PoolSettings {
            validate_target_url: "https://legacy.example/check".into(),
            validate_target_urls: vec![
                "https://one.example/check".into(),
                "https://two.example/check".into(),
            ],
            ..PoolSettings::default()
        };

        assert_eq!(
            settings.effective_validate_target_urls(),
            vec![
                "https://one.example/check".to_string(),
                "https://two.example/check".to_string()
            ]
        );
    }
}
