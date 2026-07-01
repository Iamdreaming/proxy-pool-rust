//! GeoIP lookup using MaxMind GeoLite2 database with Redis caching.

use crate::config::GeoIpSettings;
use redis::AsyncCommands;
use redis::aio::MultiplexedConnection;
use std::net::IpAddr;

/// GeoIP lookup result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GeoInfo {
    pub country: String,
    pub country_name: String,
}

/// MaxMind GeoLite2 local database lookup with Redis cache.
pub struct GeoIPLookup {
    reader: Option<maxminddb::Reader<Vec<u8>>>,
    conn: MultiplexedConnection,
    cache_ttl_secs: u64,
    domestic_countries: Vec<String>,
}

impl GeoIPLookup {
    /// Create a new GeoIP lookup. If the database cannot be loaded, all IPs
    /// will be treated as domestic (CN).
    pub fn new(conn: MultiplexedConnection, settings: &GeoIpSettings) -> Self {
        let reader = match maxminddb::Reader::open_readfile(&settings.database_path) {
            Ok(r) => {
                tracing::info!("geoip database loaded: {}", settings.database_path);
                Some(r)
            }
            Err(e) => {
                tracing::error!("failed to load geoip database: {e}");
                None
            }
        };

        Self {
            reader,
            conn,
            cache_ttl_secs: settings.cache_ttl_sec,
            domestic_countries: settings.domestic_countries.clone(),
        }
    }

    /// Look up the geographic location of a host (IP or domain).
    pub async fn lookup(&mut self, host: &str) -> GeoInfo {
        // 1. Check cache
        let cache_key = format!("geoip_cache:{host}");
        if let Ok(Some(cached)) = self.conn.get::<_, Option<String>>(&cache_key).await
            && let Ok(info) = serde_json::from_str::<GeoInfo>(&cached)
        {
            return info;
        }

        // 2. Resolve to IP if necessary
        let ip = match self.resolve_host(host) {
            Some(ip) => ip,
            None => {
                return GeoInfo {
                    country: "UNKNOWN".into(),
                    country_name: "Unknown".into(),
                };
            }
        };

        // 3. Query GeoIP database
        let result = match &self.reader {
            Some(reader) => match reader.lookup::<maxminddb::geoip2::Country>(ip) {
                Ok(response) => {
                    let country = response
                        .country
                        .as_ref()
                        .and_then(|c| c.iso_code)
                        .unwrap_or("UNKNOWN")
                        .to_string();
                    let country_name = response
                        .country
                        .as_ref()
                        .and_then(|c| c.names.as_ref()?.get("en").copied())
                        .unwrap_or("Unknown")
                        .to_string();
                    GeoInfo {
                        country,
                        country_name,
                    }
                }
                Err(_) => GeoInfo {
                    country: "UNKNOWN".into(),
                    country_name: "Unknown".into(),
                },
            },
            None => GeoInfo {
                country: "CN".into(),
                country_name: "China".into(),
            },
        };

        // 4. Cache result
        if let Ok(json) = serde_json::to_string(&result) {
            let _: Result<(), _> = self
                .conn
                .set_ex(&cache_key, &json, self.cache_ttl_secs)
                .await;
        }

        result
    }

    /// Check if a country code is overseas (not domestic).
    pub fn is_overseas(&self, country_code: &str) -> bool {
        if self.reader.is_none() {
            return false;
        }
        !self.domestic_countries.contains(&country_code.to_string()) && country_code != "UNKNOWN"
    }

    /// Resolve a hostname to an IP address.
    fn resolve_host(&self, host: &str) -> Option<IpAddr> {
        // If already an IP, return it
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Some(ip);
        }

        // DNS resolution (blocking, but for simplicity)
        use std::net::ToSocketAddrs;
        format!("{host}:0")
            .to_socket_addrs()
            .ok()?
            .next()
            .map(|a| a.ip())
    }
}
