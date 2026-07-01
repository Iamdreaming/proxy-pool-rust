//! API route definitions.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use proxy_core::models::{Protocol, Proxy};
use serde::{Deserialize, Serialize};

use crate::AppState;

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ProxyQuery {
    pub protocol: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ProxyProtocolQuery {
    pub protocol: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DeleteProxyPath {
    pub key: String, // host:port
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct StatusResponse {
    pub pool: PoolStatus,
}

#[derive(Serialize)]
pub struct PoolStatus {
    pub http: usize,
    pub https: usize,
    pub socks5: usize,
}

#[derive(Serialize)]
pub struct ProxiesResponse {
    pub protocol: String,
    pub count: usize,
    pub proxies: Vec<Proxy>,
}

#[derive(Serialize)]
pub struct SimpleResponse {
    pub status: String,
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

pub fn create_router() -> Router<AppState> {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/proxies", get(list_proxies))
        .route("/api/proxy/random", get(get_random_proxy))
        .route("/api/proxy/best", get(get_best_proxy))
        .route("/api/proxies/refresh", post(refresh_pool))
        .route("/api/proxy/{key}", delete(delete_proxy))
        .route("/api/metrics", get(metrics))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn status(State(state): State<AppState>) -> impl IntoResponse {
    let http_count = state.store.count(Protocol::Http).await.unwrap_or(0);
    let https_count = state.store.count(Protocol::Https).await.unwrap_or(0);
    let socks5_count = state.store.count(Protocol::Socks5).await.unwrap_or(0);

    Json(StatusResponse {
        pool: PoolStatus {
            http: http_count,
            https: https_count,
            socks5: socks5_count,
        },
    })
}

async fn list_proxies(
    State(state): State<AppState>,
    Query(params): Query<ProxyQuery>,
) -> impl IntoResponse {
    let protocol_str = params.protocol.as_deref().unwrap_or("http");
    let protocol = Protocol::from_str_loose(protocol_str).unwrap_or(Protocol::Http);
    let limit = params.limit.unwrap_or(20);

    match state.store.all(protocol).await {
        Ok(all) => {
            let proxies: Vec<Proxy> = all.into_iter().take(limit).collect();
            Json(ProxiesResponse {
                protocol: protocol_str.to_string(),
                count: proxies.len(),
                proxies,
            })
        }
        Err(e) => {
            tracing::error!("list_proxies error: {e}");
            Json(ProxiesResponse {
                protocol: protocol_str.to_string(),
                count: 0,
                proxies: vec![],
            })
        }
    }
}

async fn get_random_proxy(
    State(state): State<AppState>,
    Query(params): Query<ProxyProtocolQuery>,
) -> impl IntoResponse {
    let protocol_str = params.protocol.as_deref().unwrap_or("http");
    let protocol = Protocol::from_str_loose(protocol_str).unwrap_or(Protocol::Http);

    match state.store.get_random(protocol).await {
        Ok(Some(proxy)) => Json(Some(proxy)),
        Ok(None) => Json(None),
        Err(e) => {
            tracing::error!("get_random_proxy error: {e}");
            Json(None)
        }
    }
}

async fn get_best_proxy(
    State(state): State<AppState>,
    Query(params): Query<ProxyProtocolQuery>,
) -> impl IntoResponse {
    let protocol_str = params.protocol.as_deref().unwrap_or("http");
    let protocol = Protocol::from_str_loose(protocol_str).unwrap_or(Protocol::Http);

    match state.store.get_best(protocol).await {
        Ok(Some(proxy)) => Json(Some(proxy)),
        Ok(None) => Json(None),
        Err(e) => {
            tracing::error!("get_best_proxy error: {e}");
            Json(None)
        }
    }
}

async fn refresh_pool() -> impl IntoResponse {
    // TODO: trigger scheduler.run_once() via channel
    Json(SimpleResponse {
        status: "scheduled".into(),
    })
}

async fn delete_proxy(
    State(_state): State<AppState>,
    Path(_key): Path<String>,
) -> impl IntoResponse {
    // TODO: implement proxy deletion
    StatusCode::NOT_IMPLEMENTED
}

async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let http_count = state.store.count(Protocol::Http).await.unwrap_or(0);
    let https_count = state.store.count(Protocol::Https).await.unwrap_or(0);
    let socks5_count = state.store.count(Protocol::Socks5).await.unwrap_or(0);

    let lines = format!(
        "# HELP proxy_pool_size Number of proxies in pool\n\
         proxy_pool_size{{protocol=\"http\"}} {http_count}\n\
         proxy_pool_size{{protocol=\"https\"}} {https_count}\n\
         proxy_pool_size{{protocol=\"socks5\"}} {socks5_count}\n"
    );
    ([("content-type", "text/plain")], lines)
}
