//! Proxy pool data models.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::SocketAddr;

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
            country: None,
            country_name: None,
            is_overseas: false,
            warp_chain_ok: false,
            warp_chain_latency_ms: None,
            warp_chain_last_test: None,
            circuit_open: false,
            circuit_open_until: None,
            source: None,
        }
    }

    /// The proxy URL, e.g. `socks5://1.2.3.4:1080`.
    pub fn url(&self) -> String {
        format!("{}://{}:{}", self.protocol.scheme(), self.host, self.port)
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
