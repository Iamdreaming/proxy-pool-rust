//! proxy-api: REST API service for the proxy pool.

mod routes;

use proxy_core::store::ProxyStore;
use std::sync::Arc;

/// Shared application state injected into all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<ProxyStore>,
}

/// Build the axum router with all API routes.
pub fn create_app(state: AppState) -> axum::Router {
    axum::Router::new()
        .merge(routes::create_router())
        .with_state(state)
}
