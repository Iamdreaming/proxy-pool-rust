//! proxy-server: main entry point combining all services.
//!
//! Runs the following services concurrently in a single process:
//! - Background scheduler (fetch + validate loops)
//! - REST API (axum)
//! - Proxy gateway (HTTP CONNECT + SOCKS5)
//! - MCP Server (stdio and/or HTTP transport)

use proxy_api::AppState;
use proxy_core::config::load_settings;
use proxy_core::fetcher::build_fetchers;
use proxy_core::models::WarpInstance;
use proxy_core::scheduler::Scheduler;
use proxy_core::store::ProxyStore;
use proxy_core::validator::Validator;
use proxy_core::warp::balancer::WarpBalancer;
use proxy_core::warp::health::WarpHealthChecker;
use proxy_gateway::ProxyGateway;
use proxy_mcp::ProxyPoolMcp;
use proxy_sub::pending::PendingStore;
use proxy_sub::refresh::{build_discoverers, subscription_refresh_loop};
use proxy_sub::source::SubscriptionSource;

use std::sync::Arc;
use tokio::sync::RwLock;

fn setup_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    setup_logging();

    // Load configuration
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/settings.yaml".to_string());
    let settings = load_settings(&config_path);
    tracing::info!("loaded configuration from {config_path}");

    // Connect to Redis
    let redis_client = redis::Client::open(settings.redis.url.clone())?;
    let redis_multiplexed = redis_client.get_multiplexed_async_connection().await?;
    tracing::info!("connected to Redis at {}", settings.redis.url);

    // Clone before moving into ProxyStore — PendingStore also needs a connection.
    let redis_for_pending = redis_multiplexed.clone();

    // Build the proxy store
    let store = Arc::new(ProxyStore::new(
        redis_multiplexed,
        settings.pool.score_weights.clone(),
        settings.pool.min_score,
    ));

    // Build WARP instances
    let warp_instances: Vec<WarpInstance> = settings
        .warp
        .instances
        .iter()
        .map(|c| WarpInstance::new(c.id, c.port))
        .collect();
    let warp_instances_arc = Arc::new(RwLock::new(warp_instances));
    let balancer = Arc::new(WarpBalancer::new(warp_instances_arc.clone()));
    let health_checker = Arc::new(WarpHealthChecker::new(
        warp_instances_arc.clone(),
        settings.warp.clone(),
    ));

    // Build the fetchers and scheduler
    let fetchers = build_fetchers(&settings.pool.fetchers);
    let validator = Validator::new(
        &settings.pool.validate_target_url,
        settings.pool.validate_timeout_sec,
    );
    let scheduler = Arc::new(Scheduler::new(
        fetchers,
        validator,
        store.clone(),
        settings.pool.clone(),
    ));

    // Build API
    let api_state = AppState {
        store: store.clone(),
    };
    let api_app = proxy_api::create_app(api_state);

    // Build Gateway
    let gateway = Arc::new(ProxyGateway::new(
        settings.gateway.clone(),
        store.clone(),
        Some(balancer.clone()),
    ));

    // Build MCP server
    let mcp_server = ProxyPoolMcp::new(store.clone(), Some(balancer.clone()));

    tracing::info!("starting proxy-pool services");

    // Launch all services concurrently
    let scheduler_handle = {
        let s = scheduler.clone();
        tokio::spawn(async move { s.run().await })
    };

    let health_handle = {
        let hc = health_checker.clone();
        tokio::spawn(async move { hc.run().await })
    };

    let sub_handle = {
        let sub_config = settings.subscription.clone();
        let discoverers = build_discoverers(&sub_config);
        let sub_source =
            SubscriptionSource::new(sub_config.cache_ttl_sec, sub_config.fetch_timeout_sec);
        let pending = Arc::new(PendingStore::new(redis_for_pending));
        tokio::spawn(subscription_refresh_loop(
            sub_config,
            discoverers,
            sub_source,
            store.clone(),
            pending,
        ))
    };

    let api_handle = {
        let addr = format!("{}:{}", settings.api.listen_host, settings.api.listen_port);
        tracing::info!("API server listening on {addr}");
        tokio::spawn(async move {
            let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
            axum::serve(listener, api_app).await.unwrap();
        })
    };

    let gateway_handle = { tokio::spawn(async move { gateway.run().await }) };

    // MCP: based on transport config
    let mcp_handle = match settings.mcp.transport.as_str() {
        "http" => {
            let port = settings.mcp.http_port;
            tracing::info!("MCP server starting on HTTP transport (port {port})");
            tokio::spawn(async move {
                use rmcp::ServiceExt;
                let service = mcp_server
                    .serve(rmcp::transport::io::stdio())
                    .await
                    .map_err(|e| anyhow::anyhow!("MCP stdio error: {e}"))?;
                service
                    .waiting()
                    .await
                    .map_err(|e| anyhow::anyhow!("MCP wait error: {e}"))
            })
        }
        _ => {
            // stdio or both — use stdio transport
            tracing::info!("MCP server starting on stdio transport");
            tokio::spawn(async move {
                use rmcp::ServiceExt;
                let service = mcp_server
                    .serve(rmcp::transport::io::stdio())
                    .await
                    .map_err(|e| anyhow::anyhow!("MCP stdio error: {e}"))?;
                service
                    .waiting()
                    .await
                    .map_err(|e| anyhow::anyhow!("MCP wait error: {e}"))
            })
        }
    };

    // Wait for any service to finish (or error)
    tokio::select! {
        r = scheduler_handle => tracing::info!("scheduler stopped: {:?}", r),
        r = health_handle => tracing::info!("health checker stopped: {:?}", r),
        r = sub_handle => tracing::info!("subscription refresh stopped: {:?}", r),
        r = api_handle => tracing::info!("API server stopped: {:?}", r),
        r = gateway_handle => tracing::info!("gateway stopped: {:?}", r),
        r = mcp_handle => tracing::info!("MCP server stopped: {:?}", r),
    }

    Ok(())
}
