//! proxy-server: main entry point combining all services.
//!
//! Runs the following services concurrently in a single process:
//! - Background scheduler (fetch + validate loops)
//! - REST API (axum)
//! - Proxy gateway (HTTP CONNECT + SOCKS5)
//! - MCP Server (stdio and/or HTTP transport)
//! - Subscription refresh loop
//! - Xray outbound sync (if xray.enabled)

use proxy_api::AppState;
use proxy_core::circuit::CircuitBreakerConfig;
use proxy_core::config::load_settings;
use proxy_core::fetcher::build_fetchers;
use proxy_core::models::WarpInstance;
use proxy_core::pacing::ConnectionPacer;
use proxy_core::scheduler::Scheduler;
use proxy_core::store::ProxyStore;
use proxy_core::validator::Validator;
use proxy_core::warp::balancer::WarpBalancer;
use proxy_core::warp::health::WarpHealthChecker;
use proxy_gateway::ProxyGateway;
use proxy_gateway::UpstreamSelector;
use proxy_mcp::ProxyPoolMcp;
use proxy_sub::pending::PendingStore;
use proxy_sub::refresh::{build_discoverers, subscription_refresh_loop};
use proxy_sub::source::SubscriptionSource;
use proxy_xray::config_gen::ConfigGenerator;
use proxy_xray::outbound_sync::OutboundSync;
use proxy_xray::port_manager::PortManager;
use proxy_xray::process::XrayProcess;
use proxy_xray::xray_client::XrayClient;

use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

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

    let redis_for_geoip = redis_multiplexed.clone();

    // Build the proxy store
    let circuit_config = CircuitBreakerConfig::default();
    let store = Arc::new(ProxyStore::new(
        redis_multiplexed,
        settings.pool.score_weights.clone(),
        settings.pool.min_score,
        circuit_config,
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
    let validator = {
        let v = Validator::new(
            &settings.pool.validate_target_url,
            settings.pool.validate_timeout_sec,
        );
        if settings.pool.pace_rate_per_sec > 0.0 {
            v.with_pacer(Arc::new(ConnectionPacer::new(
                settings.pool.pace_rate_per_sec,
            )))
        } else {
            v
        }
    };
    let scheduler = Arc::new(Scheduler::new(
        fetchers,
        validator,
        store.clone(),
        settings.pool.clone(),
    ));

    // Build UpstreamSelector with optional Router and GeoIP
    let router = None; // TODO: load from settings.routes_path if configured
    let geoip = if settings.geoip.database_path
        != proxy_core::config::GeoIpSettings::default().database_path
        || std::path::Path::new(&settings.geoip.database_path).exists()
    {
        Some(Arc::new(Mutex::new(proxy_core::geoip::GeoIPLookup::new(
            redis_for_geoip,
            &settings.geoip,
        ))))
    } else {
        tracing::info!("geoip: database not found, skipping GeoIP-based routing");
        None
    };

    let selector = Arc::new(UpstreamSelector::new(
        store.clone(),
        Some(balancer.clone()),
        router,
        geoip,
    ));

    // Build API
    let xray_active_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let api_state = AppState {
        store: store.clone(),
        xray_active_count: xray_active_count.clone(),
    };
    let api_app = proxy_api::create_app(api_state);

    // Build Gateway (now uses UpstreamSelector)
    let gateway = Arc::new(ProxyGateway::new(
        settings.gateway.clone(),
        selector.clone(),
    ));

    // Build MCP server
    let mcp_server = ProxyPoolMcp::new(store.clone(), Some(balancer.clone()));

    tracing::info!("starting proxy-pool services");

    // --- Xray integration (conditional) ---
    let xray_sync_handle = if settings.xray.enabled {
        tracing::info!("xray integration enabled");

        // 1. Generate bootstrap config
        let xray_config_path = std::env::temp_dir().join("proxy-pool-xray-config.json");
        if let Err(e) =
            ConfigGenerator::write_bootstrap_config(&xray_config_path, settings.xray.api_port)
        {
            tracing::error!("xray: failed to write bootstrap config: {e}");
        }

        // 2. Start xray-core process
        match XrayProcess::start(
            &settings.xray.binary_path,
            &xray_config_path,
            settings.xray.api_port,
        )
        .await
        {
            Ok(_xray_process) => {
                tracing::info!("xray-core process started");
            }
            Err(e) => {
                tracing::error!("xray: failed to start process: {e}");
            }
        }

        // 3. Create port manager
        let port_manager = Arc::new(PortManager::new(
            settings.xray.port_range_start,
            settings.xray.port_range_end,
        ));

        // 4. Create gRPC client
        let xray_client = Arc::new(RwLock::new(XrayClient::new(settings.xray.api_port)));

        // 5. Create a separate PendingStore for outbound sync
        let redis_for_xray = redis_client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| anyhow::anyhow!("Redis connection for xray failed: {e}"))?;
        let pending_for_xray = PendingStore::new(redis_for_xray);

        // 6. Create outbound sync
        let outbound_sync = Arc::new(OutboundSync::new(
            pending_for_xray,
            store.clone(),
            xray_client,
            port_manager,
            settings.xray.clone(),
        ));

        // 7. Spawn outbound sync loop
        {
            let s = outbound_sync.clone();
            let counter = xray_active_count.clone();
            let interval = settings.xray.sync_interval_sec;
            Some(tokio::spawn(async move {
                loop {
                    let stats = s.sync_once().await;
                    counter.store(stats.total_active, std::sync::atomic::Ordering::Relaxed);
                    tracing::info!(
                        "outbound_sync: cycle complete -- added: {}, removed: {}, failed: {}, total_active: {}",
                        stats.added,
                        stats.removed,
                        stats.failed,
                        stats.total_active
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
                }
            }))
        }
    } else {
        tracing::info!("xray integration disabled (set xray.enabled=true to enable)");
        None
    };

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
        let pending = Arc::new(PendingStore::new(
            redis_client
                .get_multiplexed_async_connection()
                .await
                .unwrap_or_else(|e| {
                    tracing::error!("Redis connection for subscription failed: {e}");
                    panic!("Redis connection failed");
                }),
        ));
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
        _r = async { if let Some(h) = xray_sync_handle { h.await } else { std::future::pending().await } } => tracing::info!("xray sync stopped"),
    }

    Ok(())
}
