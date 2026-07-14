//! proxy-server: main entry point combining all services.
//!
//! Runs the following services concurrently in a single process:
//! - Background scheduler (fetch + validate loops)
//! - REST API (axum)
//! - Proxy gateway (HTTP CONNECT + SOCKS5)
//! - MCP Server (stdio and/or HTTP transport)
//! - Subscription refresh loop
//! - Xray outbound sync (if xray.enabled)

/// Git hash injected at build time via `GIT_HASH` env var or build-script
/// auto-detection from `git rev-parse --short HEAD`.
const GIT_HASH: &str = match option_env!("GIT_HASH") {
    Some(h) => h,
    None => "unknown",
};

use proxy_api::AppState;
use proxy_core::circuit::CircuitBreakerConfig;
use proxy_core::config::load_settings;
use proxy_core::fetcher::build_fetchers;
use proxy_core::models::WarpInstance;
use proxy_core::pacing::ConnectionPacer;
use proxy_core::router::Router;
use proxy_core::scheduler::{Scheduler, SchedulerCommand, SchedulerHandle};
use proxy_core::store::ProxyStore;
use proxy_core::validator::Validator;
use proxy_core::warp::balancer::WarpBalancer;
use proxy_core::warp::health::WarpHealthChecker;
use proxy_core::xray_status::XrayStatusRegistry;
use proxy_gateway::ProxyGateway;
use proxy_gateway::UpstreamSelector;
use proxy_sub::ops::{SubscriptionOpsHandle, subscription_ops_loop};
use proxy_sub::pending::PendingStore;
use proxy_xray::config_gen::ConfigGenerator;
use proxy_xray::outbound_sync::{OutboundSync, OutboundSyncOptions, XrayValidationPlan};
use proxy_xray::port_manager::PortManager;
use proxy_xray::process::XrayProcess;
use proxy_xray::xray_client::XrayClient;

use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, mpsc};

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
    let started_at = std::time::Instant::now();
    tracing::info!("proxy-pool-rust starting (git_hash={GIT_HASH})");

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

    // Build GeoIP lookup (used by scheduler, upstream selector, and MCP server)
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
        settings.capabilities.clone(),
        geoip.clone(),
    ));

    // Create scheduler command channel so external tasks can trigger refreshes
    let (cmd_tx, cmd_rx) = mpsc::channel::<SchedulerCommand>(8);
    let scheduler_handle =
        SchedulerHandle::with_fetcher_statuses(cmd_tx, scheduler.fetcher_statuses());

    let subscription_ops = {
        let sub_config = settings.subscription.clone();
        match redis_client.get_multiplexed_async_connection().await {
            Ok(conn) => Some(SubscriptionOpsHandle::new(
                sub_config,
                store.clone(),
                Arc::new(PendingStore::new(conn)),
            )),
            Err(e) => {
                tracing::error!(
                    "Redis connection for subscription failed: {e}; subscription refresh disabled"
                );
                None
            }
        }
    };

    // Build UpstreamSelector with optional Router and GeoIP
    let router = if let Some(ref path) = settings.routes_path {
        match Router::from_yaml(path) {
            Ok(r) => {
                tracing::info!("loaded routing rules from {path}");
                Some(Arc::new(r))
            }
            Err(e) => {
                tracing::error!("failed to load routing rules from {path}: {e}");
                None
            }
        }
    } else {
        tracing::info!("no routes_path configured, using default routing");
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
    let xray_status = settings.xray.enabled.then(XrayStatusRegistry::new);

    let api_state = AppState {
        store: store.clone(),
        xray_active_count: xray_active_count.clone(),
        xray_status: xray_status.clone(),
        scheduler_handle: scheduler_handle.clone(),
        subscription_ops: subscription_ops.clone(),
        git_hash: GIT_HASH,
        started_at,
        balancer: Some(balancer.clone()),
        route_selector: selector.clone(),
        config_path: std::path::PathBuf::from(&config_path),
    };
    let api_app = proxy_api::create_app(api_state, Some("/app/web".to_string()));

    // Build Gateway (now uses UpstreamSelector)
    let gateway = Arc::new(ProxyGateway::new(
        settings.gateway.clone(),
        selector.clone(),
    ));

    tracing::info!("starting proxy-pool services");

    // --- Xray integration (conditional) ---
    let xray_sync_handle: Option<tokio::task::JoinHandle<()>>;
    let xray_supervisor_handle: Option<tokio::task::JoinHandle<()>>;
    let mut _xray_shutdown_guard: Option<tokio::sync::watch::Sender<bool>> = None;

    if settings.xray.enabled {
        tracing::info!("xray integration enabled");

        // 1. Generate bootstrap config
        let xray_config_path = std::env::temp_dir().join("proxy-pool-xray-config.json");
        if let Err(e) =
            ConfigGenerator::write_bootstrap_config(&xray_config_path, settings.xray.api_port)
        {
            tracing::error!("xray: failed to write bootstrap config: {e}");
        }

        // 2. Start xray-core process and supervisor
        let (xray_shutdown_tx, xray_shutdown_rx) = tokio::sync::watch::channel(false);
        _xray_shutdown_guard = Some(xray_shutdown_tx);

        // Clone the shutdown receiver for the reconnect loop.
        let xray_shutdown_for_reconnect = xray_shutdown_rx.clone();

        xray_supervisor_handle = match XrayProcess::start(
            &settings.xray.binary_path,
            &xray_config_path,
            settings.xray.api_port,
        )
        .await
        {
            Ok(mut xray_process) => {
                tracing::info!("xray-core process started");
                Some(tokio::spawn(async move {
                    xray_process.supervise(xray_shutdown_rx).await;
                }))
            }
            Err(e) => {
                tracing::error!("xray: failed to start process: {e}");
                None
            }
        };

        // 3. Create port manager
        let port_manager = Arc::new(PortManager::new(
            settings.xray.port_range_start,
            settings.xray.port_range_end,
        ));

        // 4. Create gRPC client
        let xray_client = Arc::new(RwLock::new(XrayClient::new(
            settings.xray.api_port,
            &settings.xray.binary_path,
        )));

        // 4a. Connect gRPC client (may fail — reconnect loop will retry)
        if let Err(e) = xray_client.write().await.connect().await {
            tracing::warn!("xray gRPC initial connect failed: {e} (reconnect loop will retry)");
        }

        // 4b. Get connection state receiver for outbound sync
        let connected_rx = xray_client.read().await.connected_rx();

        // 4c. Clone the client for the reconnect loop (OutboundSync takes the original)
        let xray_client_for_reconnect = xray_client.clone();

        // 5. Create a separate PendingStore for outbound sync
        let redis_for_xray = redis_client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| anyhow::anyhow!("Redis connection for xray failed: {e}"))?;
        let pending_for_xray = PendingStore::new(redis_for_xray);
        let xray_validation = XrayValidationPlan::from_settings(&settings.xray, &settings.pool);

        // 6. Create outbound sync
        let outbound_sync = Arc::new(OutboundSync::new(
            pending_for_xray,
            store.clone(),
            xray_client,
            port_manager,
            OutboundSyncOptions {
                config: settings.xray.clone(),
                validation: xray_validation,
                connected_rx,
                status_registry: xray_status.clone().unwrap_or_else(XrayStatusRegistry::new),
            },
        ));

        // 7. Spawn reconnect loop
        {
            tokio::spawn(async move {
                XrayClient::reconnect_loop(xray_client_for_reconnect, xray_shutdown_for_reconnect)
                    .await;
            });
        }

        // 8. Spawn outbound sync loop
        {
            let s = outbound_sync.clone();
            let counter = xray_active_count.clone();
            xray_sync_handle = Some(tokio::spawn(async move {
                s.run(counter).await;
            }));
        }
    } else {
        tracing::info!("xray integration disabled (set xray.enabled=true to enable)");
        xray_sync_handle = None;
        xray_supervisor_handle = None;
    };

    // Launch all services concurrently
    let scheduler_task = {
        let s = scheduler.clone();
        tokio::spawn(async move { s.run(Some(cmd_rx)).await })
    };

    let health_handle = {
        let hc = health_checker.clone();
        tokio::spawn(async move { hc.run().await })
    };

    let sub_handle = {
        if let Some(ops) = subscription_ops.clone() {
            Some(tokio::spawn(subscription_ops_loop(
                settings.subscription.clone(),
                ops,
            )))
        } else {
            None
        }
    };

    let api_handle = {
        let addr = format!("{}:{}", settings.api.listen_host, settings.api.listen_port);
        tracing::info!("API server listening on {addr}");
        tokio::spawn(async move {
            match tokio::net::TcpListener::bind(&addr).await {
                Ok(listener) => {
                    if let Err(e) = axum::serve(listener, api_app).await {
                        tracing::error!("API server error on {addr}: {e}");
                    }
                }
                Err(e) => tracing::error!("API bind failed on {addr}: {e}"),
            }
        })
    };

    let gateway_handle = { tokio::spawn(async move { gateway.run().await }) };

    // MCP is now served by the standalone `proxy-mcp-server` binary (its own
    // container), decoupled so that update_service restarts do not kill it.

    // Wait for critical services only.
    // API, gateway, and scheduler are the core services; if any stops, the process should exit.
    tokio::select! {
        r = scheduler_task => tracing::error!("scheduler stopped (fatal): {:?}", r),
        r = health_handle => tracing::info!("health checker stopped: {:?}", r),
        r = async { if let Some(h) = sub_handle { h.await } else { std::future::pending().await } } => tracing::info!("subscription refresh stopped: {:?}", r),
        r = api_handle => tracing::error!("API server stopped (fatal): {:?}", r),
        r = gateway_handle => tracing::error!("gateway stopped (fatal): {:?}", r),
        _r = async { if let Some(h) = xray_supervisor_handle { h.await } else { std::future::pending().await } } => tracing::info!("xray supervisor stopped"),
        _r = async { if let Some(h) = xray_sync_handle { h.await } else { std::future::pending().await } } => tracing::info!("xray sync stopped"),
    }

    Ok(())
}
