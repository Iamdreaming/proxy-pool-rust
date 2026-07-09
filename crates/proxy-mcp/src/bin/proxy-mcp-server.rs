//! Standalone MCP server binary.
//!
//! Runs the MCP HTTP transport as its own process, decoupled from the main
//! `proxy-server`. Pool/scheduler/route/xray tools reach the main service over
//! REST (`MCP_UPSTREAM_API_URL`); store reads and GeoIP run locally against the
//! shared Redis + MMDB. `update_service` lives here so triggering a Watchtower
//! restart of the main container does not kill the MCP responder.

use proxy_core::circuit::CircuitBreakerConfig;
use proxy_core::config::load_settings;
use proxy_core::geoip::GeoIPLookup;
use proxy_core::store::ProxyStore;
use proxy_mcp::serve::serve_http;
use proxy_mcp::{ProxyPoolMcp, ProxyPoolMcpConfig};
use std::sync::Arc;
use tokio::sync::Mutex;

const GIT_HASH: &str = match option_env!("GIT_HASH") {
    Some(h) => h,
    None => "unknown",
};

fn setup_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

/// Read an env var with a fallback default.
fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    setup_logging();
    tracing::info!("proxy-mcp-server starting (git_hash={GIT_HASH})");

    let config_path = env_or("PROXY_POOL_CONFIG", "config/settings.yaml");
    let settings = load_settings(&config_path);
    tracing::info!("loaded configuration from {config_path}");

    let upstream_api_url = env_or("MCP_UPSTREAM_API_URL", "http://proxy-pool:8000");
    let http_port: u16 = env_or("MCP_HTTP_PORT", "9000").parse().unwrap_or(9000);
    tracing::info!("MCP upstream API: {upstream_api_url}; HTTP port: {http_port}");

    // Redis-backed pool store (shared with the main service).
    let redis_client = redis::Client::open(settings.redis.url.clone())?;
    let redis_conn = redis_client.get_multiplexed_async_connection().await?;
    tracing::info!("connected to Redis at {}", settings.redis.url);

    let store = Arc::new(ProxyStore::new(
        redis_conn,
        settings.pool.score_weights.clone(),
        settings.pool.min_score,
        CircuitBreakerConfig::default(),
    ));

    // Local GeoIP (MMDB + Redis cache), mirroring the main service's condition.
    let geoip = if settings.geoip.database_path
        != proxy_core::config::GeoIpSettings::default().database_path
        || std::path::Path::new(&settings.geoip.database_path).exists()
    {
        match redis_client.get_multiplexed_async_connection().await {
            Ok(geoip_conn) => Some(Arc::new(Mutex::new(GeoIPLookup::new(
                geoip_conn,
                &settings.geoip,
            )))),
            Err(e) => {
                tracing::warn!("geoip: Redis connection failed: {e}; geoip_lookup disabled");
                None
            }
        }
    } else {
        tracing::info!("geoip: database not found, geoip_lookup disabled");
        None
    };

    let mcp = ProxyPoolMcp::new(ProxyPoolMcpConfig {
        store,
        geoip,
        upstream_api_url,
    });

    serve_http(mcp, http_port).await;
    Ok(())
}
