//! proxy-api: REST API service for the proxy pool.

mod routes;

use proxy_core::route_debug::UpstreamSelector;
use proxy_core::scheduler::SchedulerHandle;
use proxy_core::store::ProxyStore;
use proxy_core::warp::WarpBalancer;
use proxy_core::xray_status::XrayStatusRegistry;
use proxy_sub::ops::SubscriptionOpsHandle;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;

/// Shared application state injected into all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<ProxyStore>,
    /// Number of active xray encrypted nodes (updated by OutboundSync).
    pub xray_active_count: Arc<AtomicUsize>,
    /// Optional lifecycle registry for xray encrypted nodes.
    pub xray_status: Option<XrayStatusRegistry>,
    /// Handle for sending commands to the background scheduler.
    pub scheduler_handle: SchedulerHandle,
    /// Optional subscription source operations handle.
    pub subscription_ops: Option<SubscriptionOpsHandle>,
    /// Git hash of the running binary (injected at build time).
    pub git_hash: &'static str,
    /// Process start time for uptime reporting.
    pub started_at: Instant,
    /// Optional WARP balancer for reporting WARP instance status.
    pub balancer: Option<Arc<WarpBalancer>>,
    /// Shared gateway route selector for route dry-run and metrics.
    pub route_selector: Arc<UpstreamSelector>,
}

/// Build the axum router with all API routes and optional web UI.
///
/// If `web_dir` is `Some(path)`, serves the SPA static files from that directory.
/// API routes are mounted at `/api/*`, everything else falls back to `index.html`.
pub fn create_app(state: AppState, web_dir: Option<String>) -> axum::Router {
    let api_routes = routes::create_router();

    let mut router = axum::Router::new().merge(api_routes).with_state(state);

    if let Some(dir) = web_dir {
        let serve_dir = tower_http::services::ServeDir::new(&dir).fallback(
            tower_http::services::ServeFile::new(std::path::Path::new(&dir).join("index.html")),
        );
        router = router.fallback_service(serve_dir);
    }

    router
}
