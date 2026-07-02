//! proxy-api: REST API service for the proxy pool.

mod routes;

use proxy_core::scheduler::SchedulerHandle;
use proxy_core::store::ProxyStore;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

/// Shared application state injected into all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<ProxyStore>,
    /// Number of active xray encrypted nodes (updated by OutboundSync).
    pub xray_active_count: Arc<AtomicUsize>,
    /// Handle for sending commands to the background scheduler.
    pub scheduler_handle: SchedulerHandle,
    /// Git hash of the running binary (injected at build time).
    pub git_hash: &'static str,
}

/// Build the axum router with all API routes.
pub fn create_app(state: AppState) -> axum::Router {
    axum::Router::new()
        .merge(routes::create_router())
        .with_state(state)
}
