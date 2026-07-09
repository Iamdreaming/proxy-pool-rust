//! Proxy pool data models.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::SocketAddr;

/// Maximum number of recent validation samples retained per proxy.
pub const QUALITY_HISTORY_LIMIT: usize = 10;

// ---------------------------------------------------------------------------
// Protocol
// ---------------------------------------------------------------------------

/// Supported proxy protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Http,
    Https,
    Socks4,
    Socks5,
}

impl Protocol {
    /// All known protocol variants.
    pub fn all() -> &'static [Protocol] {
        &[
            Protocol::Http,
            Protocol::Https,
            Protocol::Socks4,
            Protocol::Socks5,
        ]
    }

    /// Returns the scheme prefix used in URLs (e.g. "http", "socks5").
    pub fn scheme(&self) -> &'static str {
        match self {
            Protocol::Http => "http",
            Protocol::Https => "https",
            Protocol::Socks4 => "socks4",
            Protocol::Socks5 => "socks5",
        }
    }

    /// Parse from a case-insensitive string.
    pub fn from_str_loose(s: &str) -> Option<Protocol> {
        match s.to_ascii_lowercase().as_str() {
            "http" => Some(Protocol::Http),
            "https" => Some(Protocol::Https),
            "socks4" => Some(Protocol::Socks4),
            "socks5" => Some(Protocol::Socks5),
            _ => None,
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.scheme())
    }
}

// ---------------------------------------------------------------------------
// Anonymity
// ---------------------------------------------------------------------------

/// Anonymity level of a proxy as detected by the validator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Anonymity {
    Transparent,
    Anonymous,
    Elite,
}

impl Anonymity {
    /// Bonus weight used in scoring (elite=1.0, anonymous=0.5, transparent=0.0).
    pub fn bonus(&self) -> f64 {
        match self {
            Anonymity::Elite => 1.0,
            Anonymity::Anonymous => 0.5,
            Anonymity::Transparent => 0.0,
        }
    }

    /// Ordering level: Transparent=0, Anonymous=1, Elite=2.
    pub fn level(&self) -> u8 {
        match self {
            Anonymity::Transparent => 0,
            Anonymity::Anonymous => 1,
            Anonymity::Elite => 2,
        }
    }

    /// Whether this anonymity meets the given minimum level.
    pub fn meets(&self, min: Anonymity) -> bool {
        self.level() >= min.level()
    }

    pub fn from_str_loose(s: &str) -> Option<Anonymity> {
        match s.to_ascii_lowercase().as_str() {
            "transparent" => Some(Anonymity::Transparent),
            "anonymous" => Some(Anonymity::Anonymous),
            "elite" => Some(Anonymity::Elite),
            _ => None,
        }
    }
}

impl fmt::Display for Anonymity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Anonymity::Transparent => write!(f, "transparent"),
            Anonymity::Anonymous => write!(f, "anonymous"),
            Anonymity::Elite => write!(f, "elite"),
        }
    }
}

// ---------------------------------------------------------------------------
// Proxy
// ---------------------------------------------------------------------------

/// A proxy server entry stored in the pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proxy {
    pub host: String,
    pub port: u16,
    pub protocol: Protocol,
    pub latency_ms: Option<f64>,
    pub anonymity: Option<Anonymity>,
    pub last_check: Option<DateTime<Utc>>,

    // -- reliability counters --
    pub success_count: u32,
    pub fail_count: u32,
    /// Bounded recent validation history used for trend explanations.
    #[serde(default, skip_serializing_if = "QualityHistory::is_empty")]
    pub quality_history: QualityHistory,

    // -- GeoIP --
    pub country: Option<String>,
    pub country_name: Option<String>,
    /// `true` means overseas (non-CN), `false` means domestic (CN).
    pub is_overseas: bool,

    // -- WARP chain test --
    pub warp_chain_ok: bool,
    pub warp_chain_latency_ms: Option<f64>,
    pub warp_chain_last_test: Option<DateTime<Utc>>,

    // -- Circuit breaker --
    pub circuit_open: bool,
    pub circuit_open_until: Option<DateTime<Utc>>,

    // -- Source tracking --
    pub source: Option<String>,

    // -- Encrypted proxy state (xray integration) --
    /// For encrypted-protocol nodes: tracks the xray integration state.
    #[serde(default)]
    pub encrypted_state: Option<EncryptedProxyState>,
    /// The original encrypted node configuration (for recovery/re-sync on xray restart).
    #[serde(default)]
    pub encrypted_config: Option<serde_json::Value>,
}

impl Proxy {
    /// Create a minimal proxy with only host, port, and protocol.
    pub fn new(host: impl Into<String>, port: u16, protocol: Protocol) -> Self {
        Self {
            host: host.into(),
            port,
            protocol,
            latency_ms: None,
            anonymity: None,
            last_check: None,
            success_count: 0,
            fail_count: 0,
            quality_history: QualityHistory::default(),
            country: None,
            country_name: None,
            is_overseas: false,
            warp_chain_ok: false,
            warp_chain_latency_ms: None,
            warp_chain_last_test: None,
            circuit_open: false,
            circuit_open_until: None,
            source: None,
            encrypted_state: None,
            encrypted_config: None,
        }
    }

    /// The proxy URL, e.g. `socks5://1.2.3.4:1080`.
    pub fn url(&self) -> String {
        format!("{}://{}:{}", self.protocol.scheme(), self.host, self.port)
    }

    /// The URL a client uses to *dial* this proxy.
    ///
    /// For `Https` proxies this is `http://host:port`: the "https" label means
    /// the proxy supports CONNECT tunneling to TLS targets, not that the proxy
    /// endpoint itself speaks TLS. Dialing it over `https://` (as `url()` would
    /// imply) makes the client TLS-handshake the proxy port and always fails.
    /// All other protocols match `url()`.
    pub fn proxy_connect_url(&self) -> String {
        match self.protocol {
            Protocol::Https => format!("http://{}:{}", self.host, self.port),
            _ => self.url(),
        }
    }

    /// Unique identity key: `host:port`.
    pub fn key(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Unique key including protocol: `protocol:host:port`.
    pub fn dedup_key(&self) -> String {
        format!("{}:{}", self.protocol, self.key())
    }

    /// Attempt to parse `host:port` into a socket address.
    pub fn to_socket_addr(&self) -> Option<SocketAddr> {
        format!("{}:{}", self.host, self.port).parse().ok()
    }

    /// Whether this proxy is alive (not circuit-broken and has at least one
    /// success, or is brand-new).
    pub fn is_alive(&self) -> bool {
        !self.circuit_open
    }
}

// ---------------------------------------------------------------------------
// Quality history
// ---------------------------------------------------------------------------

/// One recent validation observation for a proxy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualitySample {
    /// Unix timestamp seconds when the observation happened.
    pub checked_at_unix_secs: i64,
    /// Whether the validation observation succeeded.
    pub success: bool,
    /// Rounded latency in milliseconds when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<f64>,
    /// Stable failure category or short reason when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Bounded validation history embedded in stored proxy JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct QualityHistory {
    #[serde(default)]
    pub samples: Vec<QualitySample>,
}

/// Derived recent-quality trend included in score explanations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualityTrend {
    pub recent_samples: usize,
    pub recent_success_rate: Option<f64>,
    pub recent_latency_p50: Option<f64>,
    pub recent_failures: usize,
    pub last_checked_at_unix_secs: Option<i64>,
}

impl QualityHistory {
    /// Return true when no recent observations are stored.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Append a successful validation sample while preserving the bounded size.
    pub fn record_success(&mut self, checked_at: DateTime<Utc>, latency_ms: Option<f64>) {
        self.push_sample(QualitySample {
            checked_at_unix_secs: checked_at.timestamp(),
            success: true,
            latency_ms,
            error: None,
        });
    }

    /// Append a failed validation sample while preserving the bounded size.
    pub fn record_failure(&mut self, checked_at: DateTime<Utc>, error: impl Into<String>) {
        self.push_sample(QualitySample {
            checked_at_unix_secs: checked_at.timestamp(),
            success: false,
            latency_ms: None,
            error: Some(error.into()),
        });
    }

    /// Summarize recent validation quality for score explanations.
    pub fn trend(&self) -> QualityTrend {
        let sample_count = self.samples.len();
        let successes = self.samples.iter().filter(|sample| sample.success).count();
        let failures = sample_count.saturating_sub(successes);
        let success_rate = (sample_count > 0).then_some(successes as f64 / sample_count as f64);
        let last_checked = self
            .samples
            .last()
            .map(|sample| sample.checked_at_unix_secs);

        let mut latencies: Vec<f64> = self
            .samples
            .iter()
            .filter_map(|sample| sample.latency_ms)
            .collect();
        latencies.sort_by(f64::total_cmp);
        let latency_p50 = median(&latencies);

        QualityTrend {
            recent_samples: sample_count,
            recent_success_rate: success_rate,
            recent_latency_p50: latency_p50,
            recent_failures: failures,
            last_checked_at_unix_secs: last_checked,
        }
    }

    fn push_sample(&mut self, sample: QualitySample) {
        if self.samples.last() == Some(&sample) {
            return;
        }
        self.samples.push(sample);
        let overflow = self.samples.len().saturating_sub(QUALITY_HISTORY_LIMIT);
        if overflow > 0 {
            self.samples.drain(0..overflow);
        }
    }
}

fn median(sorted_values: &[f64]) -> Option<f64> {
    match sorted_values.len() {
        0 => None,
        len if len % 2 == 1 => Some(sorted_values[len / 2]),
        len => {
            let upper = len / 2;
            Some((sorted_values[upper - 1] + sorted_values[upper]) / 2.0)
        }
    }
}

// ---------------------------------------------------------------------------
// WARP models
// ---------------------------------------------------------------------------

/// A scored WARP ingress endpoint (IP + port).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarpEndpoint {
    pub ip: String,
    pub port: u16,
    pub loss_pct: f64,
    pub latency_ms: f64,
}

impl WarpEndpoint {
    pub fn endpoint(&self) -> String {
        format!("{}:{}", self.ip, self.port)
    }
}

/// A running MicroWARP container and its currently applied endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarpInstance {
    pub id: u32,
    pub socks5_port: u16,
    pub endpoint: Option<WarpEndpoint>,
    pub healthy: bool,
    pub fail_streak: u32,
    pub last_optimized: Option<DateTime<Utc>>,
}

impl WarpInstance {
    pub fn new(id: u32, socks5_port: u16) -> Self {
        Self {
            id,
            socks5_port,
            endpoint: None,
            healthy: true,
            fail_streak: 0,
            last_optimized: None,
        }
    }
}

// -- Encrypted proxy state (Phase 2 reservation) --

/// State of an encrypted-protocol proxy node awaiting xray integration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum EncryptedProxyState {
    /// Waiting for xray instance to assign a local port.
    Pending,
    /// xray configured, local socks5 port available.
    Active { local_socks5_port: u16 },
    /// Configuration failed or xray unavailable.
    Failed,
}

// ---------------------------------------------------------------------------
// ProxyFilter
// ---------------------------------------------------------------------------

/// Composite filter for proxy queries.
///
/// All fields are optional; when `None`, that dimension is not filtered.
/// Used by both the REST API and MCP tools to let clients select proxies
/// by region, quality, anonymity, and other criteria.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyFilter {
    /// ISO country code (e.g. "US", "JP"). Exact match.
    pub country: Option<String>,
    /// Minimum anonymity level (case-insensitive).
    /// "elite" matches elite only;
    /// "anonymous" matches anonymous + elite;
    /// "transparent" matches all.
    pub anonymity: Option<String>,
    /// Maximum acceptable latency in milliseconds.
    pub max_latency: Option<f64>,
    /// `true` = overseas only, `false` = domestic only, `None` = no filter.
    pub overseas: Option<bool>,
    /// Minimum composite score (0.0..1.0).
    pub min_score: Option<f64>,
    /// Filter by source name (exact match).
    pub source: Option<String>,
    /// `true` = exclude circuit-open proxies, `false`/`None` = include all.
    pub alive: Option<bool>,
}

impl ProxyFilter {
    /// Returns `true` when every field is `None` (no filtering).
    pub fn is_empty(&self) -> bool {
        self.country.is_none()
            && self.anonymity.is_none()
            && self.max_latency.is_none()
            && self.overseas.is_none()
            && self.min_score.is_none()
            && self.source.is_none()
            && self.alive.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Proxy::proxy_connect_url --

    #[test]
    fn https_proxy_dials_over_http_scheme() {
        // "https" pool proxies are HTTP proxies that support CONNECT; the client
        // must reach them over http:// or the TLS handshake to the proxy fails.
        let proxy = Proxy::new("1.2.3.4", 8080, Protocol::Https);
        assert_eq!(proxy.url(), "https://1.2.3.4:8080");
        assert_eq!(proxy.proxy_connect_url(), "http://1.2.3.4:8080");

        let http = Proxy::new("1.2.3.4", 8080, Protocol::Http);
        assert_eq!(http.proxy_connect_url(), "http://1.2.3.4:8080");
        let socks = Proxy::new("1.2.3.4", 1080, Protocol::Socks5);
        assert_eq!(socks.proxy_connect_url(), "socks5://1.2.3.4:1080");
    }

    // -- Anonymity::level / meets --

    #[test]
    fn anonymity_level_ordering() {
        assert_eq!(Anonymity::Transparent.level(), 0);
        assert_eq!(Anonymity::Anonymous.level(), 1);
        assert_eq!(Anonymity::Elite.level(), 2);
    }

    #[test]
    fn anonymity_meets_same_level() {
        assert!(Anonymity::Transparent.meets(Anonymity::Transparent));
        assert!(Anonymity::Anonymous.meets(Anonymity::Anonymous));
        assert!(Anonymity::Elite.meets(Anonymity::Elite));
    }

    #[test]
    fn anonymity_meets_higher_includes_lower() {
        // Elite meets any minimum
        assert!(Anonymity::Elite.meets(Anonymity::Transparent));
        assert!(Anonymity::Elite.meets(Anonymity::Anonymous));
        // Anonymous meets transparent but not elite
        assert!(Anonymity::Anonymous.meets(Anonymity::Transparent));
        assert!(!Anonymity::Anonymous.meets(Anonymity::Elite));
        // Transparent only meets transparent
        assert!(!Anonymity::Transparent.meets(Anonymity::Anonymous));
        assert!(!Anonymity::Transparent.meets(Anonymity::Elite));
    }

    #[test]
    fn anonymity_from_str_loose() {
        assert_eq!(Anonymity::from_str_loose("elite"), Some(Anonymity::Elite));
        assert_eq!(
            Anonymity::from_str_loose("ANONYMOUS"),
            Some(Anonymity::Anonymous)
        );
        assert_eq!(
            Anonymity::from_str_loose("Transparent"),
            Some(Anonymity::Transparent)
        );
        assert_eq!(Anonymity::from_str_loose("unknown"), None);
    }

    // -- ProxyFilter::is_empty --

    #[test]
    fn filter_empty_when_all_none() {
        let f = ProxyFilter::default();
        assert!(f.is_empty());
    }

    #[test]
    fn filter_not_empty_when_any_set() {
        let f = ProxyFilter {
            country: Some("US".into()),
            ..Default::default()
        };
        assert!(!f.is_empty());

        let f = ProxyFilter {
            alive: Some(true),
            ..Default::default()
        };
        assert!(!f.is_empty());
    }

    #[test]
    fn proxy_deserializes_without_quality_history() {
        let json = r#"{
            "host":"1.1.1.1",
            "port":80,
            "protocol":"http",
            "latency_ms":null,
            "anonymity":null,
            "last_check":null,
            "success_count":0,
            "fail_count":0,
            "country":null,
            "country_name":null,
            "is_overseas":false,
            "warp_chain_ok":false,
            "warp_chain_latency_ms":null,
            "warp_chain_last_test":null,
            "circuit_open":false,
            "circuit_open_until":null,
            "source":null
        }"#;

        let proxy: Proxy = serde_json::from_str(json).unwrap();
        assert!(proxy.quality_history.is_empty());
    }

    #[test]
    fn quality_history_keeps_latest_samples_and_summarizes_trend() {
        let base = Utc::now();
        let mut history = QualityHistory::default();
        for i in 0..12 {
            let checked_at = base + chrono::Duration::seconds(i);
            if i % 3 == 0 {
                history.record_failure(checked_at, "timeout");
            } else {
                history.record_success(checked_at, Some((i * 10) as f64));
            }
        }

        assert_eq!(history.samples.len(), QUALITY_HISTORY_LIMIT);
        assert_eq!(
            history
                .samples
                .first()
                .map(|sample| sample.checked_at_unix_secs),
            Some((base + chrono::Duration::seconds(2)).timestamp())
        );

        let trend = history.trend();
        assert_eq!(trend.recent_samples, QUALITY_HISTORY_LIMIT);
        assert_eq!(trend.recent_failures, 3);
        assert_eq!(trend.recent_success_rate, Some(0.7));
        assert_eq!(trend.recent_latency_p50, Some(70.0));
        assert_eq!(
            trend.last_checked_at_unix_secs,
            Some((base + chrono::Duration::seconds(11)).timestamp())
        );
    }

    #[test]
    fn quality_history_deduplicates_identical_last_sample() {
        let checked_at = Utc::now();
        let mut history = QualityHistory::default();
        history.record_success(checked_at, Some(42.0));
        history.record_success(checked_at, Some(42.0));

        assert_eq!(history.samples.len(), 1);
    }
}
