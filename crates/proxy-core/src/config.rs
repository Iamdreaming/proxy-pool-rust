//! Configuration: YAML loading with defaults.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};

/// Placeholder returned by settings edit APIs for sensitive values.
pub const REDACTED_VALUE: &str = "__PROXY_POOL_REDACTED__";

/// Recommended minimum score for overseas proxy pools.
///
/// This is a reference constant for documentation and example configs; it is
/// not wired into any default or validation logic.  Operators should set
/// `pool.min_score` to this value (or a tuned equivalent) in their YAML
/// when running an overseas profile.
///
/// At this threshold, a proxy needs ≤2 s latency with decent success rate
/// and anonymity to stay in the pool.  Free-list proxies that fail most
/// targets are naturally excluded.
pub const RECOMMENDED_OVERSEAS_MIN_SCORE: f64 = 0.35;

/// Errors from strict settings read/write helpers used by operator config APIs.
#[derive(Debug, thiserror::Error)]
pub enum SettingsEditError {
    #[error("cannot read config file {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("invalid config file {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_yaml::Error,
    },
    #[error("invalid settings: {0}")]
    Validation(String),
    #[error("cannot serialize settings: {0}")]
    Serialize(#[from] serde_yaml::Error),
    #[error("cannot create config directory {path}: {source}")]
    CreateDir { path: PathBuf, source: io::Error },
    #[error("cannot write temporary config file {path}: {source}")]
    WriteTemp { path: PathBuf, source: io::Error },
    #[error("cannot replace config file {path}: {source}")]
    Replace { path: PathBuf, source: io::Error },
    #[error("invalid partial settings: {0}")]
    InvalidPartial(String),
}

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
    /// Optional timeout for xray node admission validation. Defaults to pool timeout.
    #[serde(default)]
    pub validate_timeout_sec: Option<u64>,
    /// Maximum xray admission-validation attempts per sync cycle.
    #[serde(default = "default_xray_validation_attempt_limit")]
    pub validation_attempt_limit_per_cycle: usize,
    /// Cooldown before retrying an xray node that failed validation.
    #[serde(default = "default_xray_validation_failure_cooldown")]
    pub validation_failure_cooldown_sec: u64,
    /// Optional xray-specific validation targets. Empty means use pool targets.
    #[serde(default)]
    pub validate_targets: Vec<ValidationTargetConfig>,
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
    #[serde(default)]
    pub proxifly: FetcherToggle,
    #[serde(default)]
    pub databay: FetcherToggle,
    #[serde(default)]
    pub iplocate: FetcherToggle,
    #[serde(default)]
    pub vpslab: FetcherToggle,
    #[serde(default)]
    pub monosans: FetcherToggle,
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
    #[serde(default)]
    pub validate_targets: Vec<ValidationTargetConfig>,
    #[serde(default = "default_min_score")]
    pub min_score: f64,
    #[serde(default)]
    pub score_weights: ScoreWeights,
    #[serde(default)]
    pub fetchers: FetchersConfig,
    /// Max connection attempts per second (0 = unlimited).
    #[serde(default = "default_pace_rate")]
    pub pace_rate_per_sec: f64,
    /// Admission policy for multi-target validation.
    #[serde(default)]
    pub target_admission: TargetAdmissionConfig,
}

/// Structured proxy validation target configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationTargetConfig {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub expected_statuses: Vec<u16>,
}

impl ValidationTargetConfig {
    /// Build a target with default successful-status handling.
    pub fn from_url(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            expected_statuses: Vec::new(),
        }
    }
}

/// Config-driven admission policy for multi-target validation.
///
/// Maps 1:1 to [`crate::validator::TargetAdmission`] but lives in the config
/// layer so it can be deserialized from YAML and defaulted via serde.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TargetAdmissionConfig {
    /// Every target must pass for the proxy to be admitted.
    Strict,
    /// At least one target passing admits the proxy (default).
    #[default]
    Quorum,
}

impl PoolSettings {
    /// Return validation targets with backward-compatible single-target fallback.
    pub fn effective_validate_target_urls(&self) -> Vec<String> {
        if self.validate_target_urls.is_empty() {
            return vec![self.validate_target_url.clone()];
        }
        self.validate_target_urls.clone()
    }

    /// Return structured validation targets with legacy URL field fallback.
    pub fn effective_validate_targets(&self) -> Vec<ValidationTargetConfig> {
        if !self.validate_targets.is_empty() {
            return self.validate_targets.clone();
        }
        self.effective_validate_target_urls()
            .into_iter()
            .map(ValidationTargetConfig::from_url)
            .collect()
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

/// Configuration for the LLM web-search (grok) discoverer.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchDiscoverConfig {
    /// Whether the search discoverer is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// MCP streamable-HTTP endpoint URL of the grok search service.
    #[serde(default)]
    pub mcp_url: String,
    /// Bearer auth token. If empty, falls back to the SEARCH_MCP_TOKEN env var.
    #[serde(default)]
    pub auth_token: String,
    /// MCP `web_search` tool name. If empty, defaults to `grok-search-web_search`.
    #[serde(default)]
    pub tool_name: String,
    /// Search queries to run. If empty, built-in default queries are used.
    #[serde(default)]
    pub queries: Vec<String>,
    /// Maximum number of queries to run per discovery cycle.
    #[serde(default = "default_search_max_queries")]
    pub max_queries: usize,
}

fn default_search_max_queries() -> usize {
    3
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
    pub search: SearchDiscoverConfig,
    #[serde(default)]
    pub aggregators: Vec<AggregatorEntryConfig>,
    #[serde(default)]
    pub telegram: TelegramDiscoverConfig,
    /// Airport (VPN panel) auto-discovery configuration.
    #[serde(default)]
    pub airport: AirportDiscoverConfig,
    /// Airport (VPN panel) auto check-in and renewal configuration.
    #[serde(default)]
    pub checkin: CheckinConfig,
    #[serde(default = "default_sub_interval")]
    pub refresh_interval_sec: u64,
    #[serde(default = "default_sub_timeout")]
    pub fetch_timeout_sec: u64,
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_sec: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelegramDiscoverConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub channels: Vec<TelegramChannelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramChannelConfig {
    /// Channel name (the `{channel}` segment in `t.me/s/{channel}`).
    pub name: String,
    #[serde(default = "default_telegram_pages")]
    pub pages: u32,
    #[serde(default)]
    pub include: String,
    #[serde(default)]
    pub exclude: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Aggregator site that lists candidate airport (VPN panel) domains.
///
/// Used by the airport auto-discovery discoverer to seed its crawl for
/// free-panel registration targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatorSiteConfig {
    /// URL of the aggregator page that lists airport domains.
    pub url: String,
    /// Response format: `html`, `json`, or `text`. Defaults to `html`.
    #[serde(default = "default_agg_site_format")]
    pub format: String,
}

/// Configuration for the airport auto-discovery discoverer.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AirportDiscoverConfig {
    /// Whether airport auto-discovery is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Aggregator sites that list candidate airport domains.
    #[serde(default)]
    pub aggregator_sites: Vec<AggregatorSiteConfig>,
    /// Base URL of the Cloudflare Worker temp-email service.
    #[serde(default)]
    pub cloudflare_worker_url: String,
    /// Optional admin auth token for the temp-email worker.
    #[serde(default)]
    pub cloudflare_admin_auth: Option<String>,
    /// Email domain to request from the temp-email worker. Empty string lets the
    /// worker pick its default domain. Must be a domain the worker owns.
    #[serde(default)]
    pub cloudflare_email_domain: String,
    /// Maximum number of airport registrations to run concurrently.
    #[serde(default = "default_airport_concurrent")]
    pub max_concurrent: usize,
}

/// Configuration for airport (VPN panel) auto check-in and renewal.
///
/// When enabled, the subscription ops loop periodically POSTs each registered
/// airport's `/user/checkin` endpoint and re-orders the free plan when traffic
/// or time is running low.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckinConfig {
    /// Whether airport auto check-in is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Interval in seconds between check-in cycles.
    #[serde(default = "default_checkin_interval")]
    pub interval_sec: u64,
}

impl Default for CheckinConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_sec: default_checkin_interval(),
        }
    }
}

fn default_telegram_pages() -> u32 {
    1
}

fn default_agg_site_format() -> String {
    "html".into()
}

fn default_airport_concurrent() -> usize {
    3
}

fn default_checkin_interval() -> u64 {
    86400
}

// ---------------------------------------------------------------------------
// Capability tagging
// ---------------------------------------------------------------------------

/// A single capability target probed during capability tagging.
///
/// Stored under `capabilities.targets` in settings. `tag` must match a
/// `CapabilityTag` variant name (e.g. `"chat_gpt"`, `"openai"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityTargetConfig {
    /// Human-readable name for logging.
    pub name: String,
    /// URL to probe through the candidate proxy.
    pub url: String,
    /// HTTP status expected on a successful probe.
    pub expected_status: u16,
    /// Capability tag assigned on success (matches a `CapabilityTag`).
    pub tag: String,
}

/// Configuration for node capability tagging.
///
/// When enabled, the scheduler probes top-K proxies against the configured
/// targets and tags proxies that satisfy each target. A failed probe never
/// removes an existing tag; tags are only added on a successful probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityConfig {
    /// Master switch for capability tagging.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Probe targets during the normal validate pass.
    #[serde(default)]
    pub test_on_validate: bool,
    /// Probe targets during the revalidate pass.
    #[serde(default = "default_true")]
    pub test_on_revalidate: bool,
    /// How many top candidates to probe per protocol.
    #[serde(default = "default_capability_top_k")]
    pub top_k: usize,
    /// Capability targets to probe.
    #[serde(default = "default_capability_targets")]
    pub targets: Vec<CapabilityTargetConfig>,
}

impl Default for CapabilityConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            test_on_validate: false,
            test_on_revalidate: true,
            top_k: default_capability_top_k(),
            targets: default_capability_targets(),
        }
    }
}

fn default_capability_top_k() -> usize {
    8
}

fn default_capability_targets() -> Vec<CapabilityTargetConfig> {
    vec![
        CapabilityTargetConfig {
            name: "ChatGPT".into(),
            url: "https://chat.openai.com/favicon.ico".into(),
            expected_status: 200,
            tag: "chat_gpt".into(),
        },
        CapabilityTargetConfig {
            name: "OpenAI".into(),
            url: "https://api.openai.com/v1/engines".into(),
            expected_status: 401,
            tag: "openai".into(),
        },
    ]
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
    /// Node capability tagging configuration.
    #[serde(default)]
    pub capabilities: CapabilityConfig,
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

/// Strictly read settings for an operator edit surface.
///
/// Missing files still produce defaults, matching startup behavior. Read or
/// parse failures are returned so an edit UI cannot accidentally overwrite a
/// broken config file with defaults.
pub fn read_settings_for_edit(path: impl AsRef<Path>) -> Result<Settings, SettingsEditError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(Settings::default());
    }
    let text = std::fs::read_to_string(path).map_err(|source| SettingsEditError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_yaml::from_str(&text).map_err(|source| SettingsEditError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Return a display-safe settings clone plus field paths replaced by placeholders.
pub fn redact_settings(settings: &Settings) -> (Settings, Vec<String>) {
    let mut redacted = settings.clone();
    let mut fields = Vec::new();

    if !redacted.redis.url.is_empty() {
        redacted.redis.url = REDACTED_VALUE.into();
        fields.push("redis.url".into());
    }

    if matches!(
        redacted.subscription.github.token.as_deref(),
        Some(token) if !token.is_empty()
    ) {
        redacted.subscription.github.token = Some(REDACTED_VALUE.into());
        fields.push("subscription.github.token".into());
    }

    (redacted, fields)
}

/// Merge redacted placeholders in a submitted settings object with current values.
pub fn merge_redacted_settings(mut submitted: Settings, current: &Settings) -> Settings {
    if submitted.redis.url == REDACTED_VALUE {
        submitted.redis.url = current.redis.url.clone();
    }

    if matches!(
        submitted.subscription.github.token.as_deref(),
        Some(token) if token == REDACTED_VALUE
    ) {
        submitted.subscription.github.token = current.subscription.github.token.clone();
    }

    submitted
}

/// Merge a partial JSON value into the current settings.
///
/// For each top-level key present in `partial`, the corresponding section in
/// `current` is replaced entirely. Keys not present in `partial` are left
/// unchanged. Returns the merged `Settings` ready for validation and persistence.
pub fn merge_settings_partial(
    current: &Settings,
    partial: Value,
) -> Result<Settings, SettingsEditError> {
    let Value::Object(partial_map) = partial else {
        return Err(SettingsEditError::InvalidPartial(
            "partial must be a JSON object".into(),
        ));
    };

    let mut current_value =
        serde_json::to_value(current).expect("Settings serialization should not fail");
    let Value::Object(ref mut current_map) = current_value else {
        unreachable!("Settings should serialize to a JSON object");
    };

    for (key, value) in partial_map {
        // Only replace keys that exist in current settings.
        if current_map.contains_key(&key) {
            current_map.insert(key, value);
        }
        // Unknown top-level keys are silently ignored.
    }

    serde_json::from_value(current_value)
        .map_err(|e| SettingsEditError::Validation(format!("merged settings are invalid: {e}")))
}

/// Compare two settings and return the list of top-level section names that differ.
pub fn settings_changed_sections(old: &Settings, new: &Settings) -> Vec<String> {
    let old_value = serde_json::to_value(old).expect("Settings serialization should not fail");
    let new_value = serde_json::to_value(new).expect("Settings serialization should not fail");

    let Value::Object(old_map) = old_value else {
        unreachable!("Settings should serialize to a JSON object");
    };
    let Value::Object(new_map) = new_value else {
        unreachable!("Settings should serialize to a JSON object");
    };

    let mut changed: Vec<String> = Vec::new();
    for key in old_map.keys().chain(new_map.keys()).collect::<Vec<_>>() {
        if !changed.contains(key) && old_map.get(key) != new_map.get(key) {
            changed.push(key.clone());
        }
    }

    changed
}

/// Validate settings before writing them from an operator edit surface.
pub fn validate_settings(settings: &Settings) -> Result<(), SettingsEditError> {
    validate_port("gateway.listen_port", settings.gateway.listen_port)?;
    validate_port("api.listen_port", settings.api.listen_port)?;
    validate_port("mcp.http_port", settings.mcp.http_port)?;
    validate_port("xray.api_port", settings.xray.api_port)?;
    validate_port("xray.port_range_start", settings.xray.port_range_start)?;
    validate_port("xray.port_range_end", settings.xray.port_range_end)?;

    if settings.xray.port_range_start > settings.xray.port_range_end {
        return Err(SettingsEditError::Validation(
            "xray.port_range_start must be <= xray.port_range_end".into(),
        ));
    }

    validate_non_empty("redis.url", &settings.redis.url)?;
    validate_non_empty(
        "pool.validate_target_url",
        &settings.pool.validate_target_url,
    )?;
    for (idx, url) in settings.pool.validate_target_urls.iter().enumerate() {
        validate_non_empty(&format!("pool.validate_target_urls[{idx}]"), url)?;
    }
    for (idx, target) in settings.pool.validate_targets.iter().enumerate() {
        validate_non_empty(&format!("pool.validate_targets[{idx}].url"), &target.url)?;
    }
    for (idx, target) in settings.xray.validate_targets.iter().enumerate() {
        validate_non_empty(&format!("xray.validate_targets[{idx}].url"), &target.url)?;
    }
    for (idx, url) in settings.subscription.urls.iter().enumerate() {
        validate_non_empty(&format!("subscription.urls[{idx}]"), url)?;
    }
    for (idx, aggregator) in settings.subscription.aggregators.iter().enumerate() {
        validate_non_empty(
            &format!("subscription.aggregators[{idx}].url"),
            &aggregator.url,
        )?;
    }

    validate_non_negative_finite("pool.min_score", settings.pool.min_score)?;
    if settings.pool.min_score > 1.0 {
        return Err(SettingsEditError::Validation(
            "pool.min_score must be <= 1.0".into(),
        ));
    }
    validate_non_negative_finite("pool.pace_rate_per_sec", settings.pool.pace_rate_per_sec)?;
    validate_non_negative_finite(
        "pool.score_weights.latency",
        settings.pool.score_weights.latency,
    )?;
    validate_non_negative_finite(
        "pool.score_weights.success",
        settings.pool.score_weights.success,
    )?;
    validate_non_negative_finite(
        "pool.score_weights.anonymity",
        settings.pool.score_weights.anonymity,
    )?;
    validate_non_negative_finite(
        "warp.optimizer.max_loss_pct",
        settings.warp.optimizer.max_loss_pct,
    )?;
    if settings.warp.optimizer.max_loss_pct > 100.0 {
        return Err(SettingsEditError::Validation(
            "warp.optimizer.max_loss_pct must be <= 100.0".into(),
        ));
    }

    if settings.pool.validate_concurrency == 0 {
        return Err(SettingsEditError::Validation(
            "pool.validate_concurrency must be greater than 0".into(),
        ));
    }
    if settings.pool.validate_timeout_sec == 0 {
        return Err(SettingsEditError::Validation(
            "pool.validate_timeout_sec must be greater than 0".into(),
        ));
    }
    if settings.xray.validate_timeout_sec == Some(0) {
        return Err(SettingsEditError::Validation(
            "xray.validate_timeout_sec must be greater than 0 when set".into(),
        ));
    }
    if settings.xray.validation_attempt_limit_per_cycle == 0 {
        return Err(SettingsEditError::Validation(
            "xray.validation_attempt_limit_per_cycle must be greater than 0".into(),
        ));
    }
    if settings.xray.validation_failure_cooldown_sec == 0 {
        return Err(SettingsEditError::Validation(
            "xray.validation_failure_cooldown_sec must be greater than 0".into(),
        ));
    }
    if settings.subscription.fetch_timeout_sec == 0 {
        return Err(SettingsEditError::Validation(
            "subscription.fetch_timeout_sec must be greater than 0".into(),
        ));
    }

    Ok(())
}

/// Merge, validate, and persist submitted settings for an operator edit surface.
pub fn write_settings_for_edit(
    path: impl AsRef<Path>,
    submitted: Settings,
) -> Result<Settings, SettingsEditError> {
    let path = path.as_ref();
    let current = read_settings_for_edit(path)?;
    let settings = merge_redacted_settings(submitted, &current);
    validate_settings(&settings)?;
    let yaml = serde_yaml::to_string(&settings)?;
    let _: Settings = serde_yaml::from_str(&yaml)?;
    write_settings_yaml(path, &yaml)?;
    Ok(settings)
}

fn validate_non_empty(field: &str, value: &str) -> Result<(), SettingsEditError> {
    if value.trim().is_empty() {
        return Err(SettingsEditError::Validation(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

fn validate_non_negative_finite(field: &str, value: f64) -> Result<(), SettingsEditError> {
    if !value.is_finite() {
        return Err(SettingsEditError::Validation(format!(
            "{field} must be finite"
        )));
    }
    if value < 0.0 {
        return Err(SettingsEditError::Validation(format!(
            "{field} must be >= 0"
        )));
    }
    Ok(())
}

fn validate_port(field: &str, value: u16) -> Result<(), SettingsEditError> {
    if value == 0 {
        return Err(SettingsEditError::Validation(format!(
            "{field} must be greater than 0"
        )));
    }
    Ok(())
}

fn write_settings_yaml(path: &Path, yaml: &str) -> Result<(), SettingsEditError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|source| SettingsEditError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let temp_path = sibling_path(path, "tmp");
    std::fs::write(&temp_path, yaml).map_err(|source| SettingsEditError::WriteTemp {
        path: temp_path.clone(),
        source,
    })?;

    replace_settings_file(path, &temp_path)
}

fn replace_settings_file(path: &Path, temp_path: &Path) -> Result<(), SettingsEditError> {
    if !path.exists() {
        return std::fs::rename(temp_path, path).map_err(|source| SettingsEditError::Replace {
            path: path.to_path_buf(),
            source,
        });
    }

    let backup_path = sibling_path(path, "bak");
    let _ = std::fs::remove_file(&backup_path);
    std::fs::copy(path, &backup_path).map_err(|source| SettingsEditError::Replace {
        path: path.to_path_buf(),
        source,
    })?;

    if let Err(source) = std::fs::remove_file(path) {
        let _ = std::fs::remove_file(temp_path);
        let _ = std::fs::remove_file(&backup_path);
        return Err(SettingsEditError::Replace {
            path: path.to_path_buf(),
            source,
        });
    }

    match std::fs::rename(temp_path, path) {
        Ok(()) => {
            let _ = std::fs::remove_file(&backup_path);
            Ok(())
        }
        Err(source) => {
            let _ = std::fs::copy(&backup_path, path);
            let _ = std::fs::remove_file(temp_path);
            let _ = std::fs::remove_file(&backup_path);
            Err(SettingsEditError::Replace {
                path: path.to_path_buf(),
                source,
            })
        }
    }
}

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("settings.yaml");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    path.with_file_name(format!(
        ".{file_name}.{}.{}.{}",
        std::process::id(),
        stamp,
        suffix
    ))
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
    0.3
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
fn default_xray_validation_attempt_limit() -> usize {
    50
}
fn default_xray_validation_failure_cooldown() -> u64 {
    600
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

    #[test]
    fn effective_validate_targets_falls_back_to_single_target() {
        let settings = PoolSettings {
            validate_target_url: "https://one.example/check".into(),
            validate_target_urls: vec![],
            validate_targets: vec![],
            ..PoolSettings::default()
        };

        assert_eq!(
            settings.effective_validate_targets(),
            vec![ValidationTargetConfig::from_url(
                "https://one.example/check"
            )]
        );
    }

    #[test]
    fn effective_validate_targets_prefers_legacy_url_list() {
        let settings = PoolSettings {
            validate_target_url: "https://legacy.example/check".into(),
            validate_target_urls: vec![
                "https://one.example/check".into(),
                "https://two.example/check".into(),
            ],
            validate_targets: vec![],
            ..PoolSettings::default()
        };

        assert_eq!(
            settings.effective_validate_targets(),
            vec![
                ValidationTargetConfig::from_url("https://one.example/check"),
                ValidationTargetConfig::from_url("https://two.example/check")
            ]
        );
    }

    #[test]
    fn effective_validate_targets_prefers_structured_targets() {
        let settings = PoolSettings {
            validate_target_url: "https://legacy.example/check".into(),
            validate_target_urls: vec!["https://one.example/check".into()],
            validate_targets: vec![ValidationTargetConfig {
                url: "https://api.openai.com/v1/models".into(),
                expected_statuses: vec![401],
            }],
            ..PoolSettings::default()
        };

        assert_eq!(
            settings.effective_validate_targets(),
            vec![ValidationTargetConfig {
                url: "https://api.openai.com/v1/models".into(),
                expected_statuses: vec![401],
            }]
        );
    }

    #[test]
    fn target_admission_config_serde_roundtrip() {
        // Default is Quorum
        let default = TargetAdmissionConfig::default();
        assert_eq!(default, TargetAdmissionConfig::Quorum);

        // Explicit quorum
        let q: TargetAdmissionConfig = serde_yaml::from_str("quorum").unwrap();
        assert_eq!(q, TargetAdmissionConfig::Quorum);

        // Explicit strict
        let s: TargetAdmissionConfig = serde_yaml::from_str("strict").unwrap();
        assert_eq!(s, TargetAdmissionConfig::Strict);

        // Round-trip
        let yaml = serde_yaml::to_string(&TargetAdmissionConfig::Strict).unwrap();
        assert!(yaml.contains("strict"));

        // Missing field defaults to Quorum in PoolSettings
        let pool: PoolSettings = serde_yaml::from_str("{}").unwrap();
        assert_eq!(pool.target_admission, TargetAdmissionConfig::Quorum);
    }

    #[test]
    fn xray_settings_validation_fields_have_safe_defaults() {
        let settings = XraySettings::default();

        assert_eq!(settings.validate_timeout_sec, None);
        assert_eq!(settings.validation_attempt_limit_per_cycle, 50);
        assert_eq!(settings.validation_failure_cooldown_sec, 600);
        assert!(settings.validate_targets.is_empty());
    }

    #[test]
    fn validate_settings_rejects_invalid_xray_validation_fields() {
        let mut settings = Settings::default();
        settings.xray.validate_timeout_sec = Some(0);
        let error = validate_settings(&settings).unwrap_err();
        assert!(error.to_string().contains("xray.validate_timeout_sec"));

        let mut settings = Settings::default();
        settings.xray.validation_attempt_limit_per_cycle = 0;
        let error = validate_settings(&settings).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("xray.validation_attempt_limit_per_cycle")
        );

        let mut settings = Settings::default();
        settings.xray.validation_failure_cooldown_sec = 0;
        let error = validate_settings(&settings).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("xray.validation_failure_cooldown_sec")
        );

        let mut settings = Settings::default();
        settings.xray.validate_targets = vec![ValidationTargetConfig::from_url("   ")];
        let error = validate_settings(&settings).unwrap_err();
        assert!(error.to_string().contains("xray.validate_targets[0].url"));
    }

    #[test]
    fn fetchers_config_defaults_enable_public_sources() {
        let config = FetchersConfig::default();

        assert!(config.proxifly.enabled);
        assert!(config.databay.enabled);
        assert!(config.iplocate.enabled);
        assert!(config.vpslab.enabled);
        assert!(config.monosans.enabled);
        assert!(config.proxifly.use_mirror);
        assert!(config.databay.use_mirror);
        assert!(config.iplocate.use_mirror);
        assert!(config.vpslab.use_mirror);
        assert!(config.monosans.use_mirror);
    }

    #[test]
    fn fetchers_config_can_disable_public_sources() {
        let config: FetchersConfig = serde_yaml::from_str(
            r#"
proxifly: { enabled: false }
databay: { enabled: false }
iplocate: { enabled: false }
vpslab: { enabled: false }
monosans: { enabled: false }
"#,
        )
        .unwrap();

        assert!(!config.proxifly.enabled);
        assert!(!config.databay.enabled);
        assert!(!config.iplocate.enabled);
        assert!(!config.vpslab.enabled);
        assert!(!config.monosans.enabled);
    }

    #[test]
    fn redact_settings_hides_sensitive_values() {
        let settings = Settings {
            redis: RedisSettings {
                url: "redis://:secret@redis:6379/0".into(),
            },
            subscription: SubscriptionConfig {
                github: GitHubDiscoverConfig {
                    token: Some("github-secret".into()),
                    ..GitHubDiscoverConfig::default()
                },
                ..SubscriptionConfig::default()
            },
            ..Settings::default()
        };

        let (redacted, fields) = redact_settings(&settings);

        assert_eq!(redacted.redis.url, REDACTED_VALUE);
        assert_eq!(
            redacted.subscription.github.token.as_deref(),
            Some(REDACTED_VALUE)
        );
        assert_eq!(
            fields,
            vec![
                "redis.url".to_string(),
                "subscription.github.token".to_string()
            ]
        );
    }

    #[test]
    fn merge_redacted_settings_preserves_current_sensitive_values() {
        let current = Settings {
            redis: RedisSettings {
                url: "redis://:secret@redis:6379/0".into(),
            },
            subscription: SubscriptionConfig {
                github: GitHubDiscoverConfig {
                    token: Some("github-secret".into()),
                    ..GitHubDiscoverConfig::default()
                },
                ..SubscriptionConfig::default()
            },
            ..Settings::default()
        };
        let submitted = Settings {
            redis: RedisSettings {
                url: REDACTED_VALUE.into(),
            },
            subscription: SubscriptionConfig {
                github: GitHubDiscoverConfig {
                    token: Some(REDACTED_VALUE.into()),
                    ..GitHubDiscoverConfig::default()
                },
                ..SubscriptionConfig::default()
            },
            pool: PoolSettings {
                fetch_interval_sec: 123,
                ..PoolSettings::default()
            },
            ..Settings::default()
        };

        let merged = merge_redacted_settings(submitted, &current);

        assert_eq!(merged.redis.url, "redis://:secret@redis:6379/0");
        assert_eq!(
            merged.subscription.github.token.as_deref(),
            Some("github-secret")
        );
        assert_eq!(merged.pool.fetch_interval_sec, 123);
    }

    #[test]
    fn write_settings_for_edit_preserves_redacted_values() {
        let path = temp_config_path("preserve_redacted");
        let current = Settings {
            redis: RedisSettings {
                url: "redis://:secret@redis:6379/0".into(),
            },
            subscription: SubscriptionConfig {
                github: GitHubDiscoverConfig {
                    token: Some("github-secret".into()),
                    ..GitHubDiscoverConfig::default()
                },
                ..SubscriptionConfig::default()
            },
            ..Settings::default()
        };
        std::fs::write(&path, serde_yaml::to_string(&current).unwrap()).unwrap();

        let (mut submitted, _) = redact_settings(&current);
        submitted.pool.fetch_interval_sec = 123;
        let saved = write_settings_for_edit(&path, submitted).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(saved.redis.url, "redis://:secret@redis:6379/0");
        assert_eq!(
            saved.subscription.github.token.as_deref(),
            Some("github-secret")
        );
        assert_eq!(saved.pool.fetch_interval_sec, 123);
        assert!(raw.contains("redis://:secret@redis:6379/0"));
        assert!(raw.contains("github-secret"));
        assert!(!raw.contains(REDACTED_VALUE));
    }

    #[test]
    fn write_settings_for_edit_rejects_invalid_without_overwriting() {
        let path = temp_config_path("reject_invalid");
        let current = Settings {
            redis: RedisSettings {
                url: "redis://redis:6379/0".into(),
            },
            ..Settings::default()
        };
        let original = serde_yaml::to_string(&current).unwrap();
        std::fs::write(&path, &original).unwrap();

        let submitted = Settings {
            redis: RedisSettings { url: "".into() },
            ..Settings::default()
        };
        let result = write_settings_for_edit(&path, submitted);
        let after = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(matches!(result, Err(SettingsEditError::Validation(_))));
        assert_eq!(after, original);
    }

    fn temp_config_path(name: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "proxy_pool_rust_{name}_{}_{}.yaml",
            std::process::id(),
            stamp
        ))
    }

    #[test]
    fn test_merge_settings_partial_subscription() {
        let current = Settings::default();
        let partial = serde_json::json!({
            "subscription": {
                "refresh_interval_sec": 7200,
                "fetch_timeout_sec": 45
            }
        });
        let merged = merge_settings_partial(&current, partial).unwrap();
        assert_eq!(merged.subscription.refresh_interval_sec, 7200);
        assert_eq!(merged.subscription.fetch_timeout_sec, 45);
        // Other sections should remain at defaults.
        assert_eq!(
            merged.pool.fetch_interval_sec,
            current.pool.fetch_interval_sec
        );
    }

    #[test]
    fn test_merge_settings_partial_empty() {
        let current = Settings::default();
        let partial = serde_json::json!({});
        let merged = merge_settings_partial(&current, partial).unwrap();
        assert_eq!(
            serde_json::to_value(&merged).unwrap(),
            serde_json::to_value(&current).unwrap()
        );
    }

    #[test]
    fn test_merge_settings_partial_unknown_key() {
        let current = Settings::default();
        let partial = serde_json::json!({"unknown_section": {"foo": 1}});
        let merged = merge_settings_partial(&current, partial).unwrap();
        assert_eq!(
            serde_json::to_value(&merged).unwrap(),
            serde_json::to_value(&current).unwrap()
        );
    }

    #[test]
    fn test_settings_changed_sections_same() {
        let settings = Settings::default();
        let changed = settings_changed_sections(&settings, &settings);
        assert!(changed.is_empty());
    }

    #[test]
    fn test_settings_changed_sections_different() {
        let old = Settings::default();
        let mut new = Settings::default();
        new.subscription.refresh_interval_sec = 9999;
        let changed = settings_changed_sections(&old, &new);
        assert_eq!(changed, vec!["subscription"]);
    }
}
