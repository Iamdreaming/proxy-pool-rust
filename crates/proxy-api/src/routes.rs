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
use std::sync::atomic::Ordering;

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
    pub version: &'static str,
    pub git_hash: &'static str,
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

#[derive(Serialize)]
pub struct XrayStatusResponse {
    pub active_nodes: usize,
}

#[derive(Serialize)]
pub struct RefreshResponse {
    pub status: String,
    pub fetched: usize,
    pub validated: usize,
    pub stored: usize,
    pub errors: usize,
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
        .route("/api/xray/status", get(xray_status))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a JSON status response with the given HTTP status code.
fn json_status(code: StatusCode, msg: impl Into<String>) -> axum::response::Response {
    (code, Json(SimpleResponse { status: msg.into() })).into_response()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn status(State(state): State<AppState>) -> impl IntoResponse {
    let http_count = state.store.count(Protocol::Http).await.unwrap_or(0);
    let https_count = state.store.count(Protocol::Https).await.unwrap_or(0);
    let socks5_count = state.store.count(Protocol::Socks5).await.unwrap_or(0);

    Json(StatusResponse {
        version: env!("CARGO_PKG_VERSION"),
        git_hash: state.git_hash,
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

async fn refresh_pool(State(state): State<AppState>) -> impl IntoResponse {
    match state.scheduler_handle.refresh().await {
        Ok(result) => Json(RefreshResponse {
            status: "ok".into(),
            fetched: result.fetched,
            validated: result.validated,
            stored: result.stored,
            errors: result.errors,
        }),
        Err(e) => Json(RefreshResponse {
            status: format!("error: {e}"),
            fetched: 0,
            validated: 0,
            stored: 0,
            errors: 0,
        }),
    }
}

async fn delete_proxy(State(state): State<AppState>, Path(key): Path<String>) -> impl IntoResponse {
    // Parse key format: "protocol:host:port"
    let parts: Vec<&str> = key.splitn(3, ':').collect();
    if parts.len() != 3 {
        return json_status(StatusCode::BAD_REQUEST, "invalid key format, expected protocol:host:port");
    }
    let protocol = match Protocol::from_str_loose(parts[0]) {
        Some(p) => p,
        None => return json_status(StatusCode::BAD_REQUEST, "invalid protocol"),
    };
    let port: u16 = match parts[2].parse() {
        Ok(p) => p,
        Err(_) => return json_status(StatusCode::BAD_REQUEST, "invalid port"),
    };
    let proxy = Proxy::new(parts[1], port, protocol);

    match state.store.remove(&proxy).await {
        Ok(true) => json_status(StatusCode::OK, "ok"),
        Ok(false) => json_status(StatusCode::NOT_FOUND, "proxy not found"),
        Err(e) => json_status(StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")),
    }
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

async fn xray_status(State(state): State<AppState>) -> impl IntoResponse {
    let active = state.xray_active_count.load(Ordering::Relaxed);
    Json(XrayStatusResponse {
        active_nodes: active,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_delete_key_valid() {
        // Test the key parsing logic used in delete_proxy
        let key = "http:1.2.3.4:8080";
        let parts: Vec<&str> = key.splitn(3, ':').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "http");
        assert_eq!(parts[1], "1.2.3.4");
        assert_eq!(parts[2], "8080");
    }

    #[test]
    fn test_parse_delete_key_invalid_no_colon() {
        let key = "invalid";
        let parts: Vec<&str> = key.splitn(3, ':').collect();
        assert_ne!(parts.len(), 3);
    }

    #[test]
    fn test_parse_delete_key_ipv6() {
        // IPv6 addresses contain colons -- splitn(3, ':') splits on first two colons.
        // This is a known limitation: IPv6 hosts in the key format are not supported.
        // The key format "protocol:host:port" assumes IPv4 or domain host.
        let key = "socks5:[::1]:1080";
        let parts: Vec<&str> = key.splitn(3, ':').collect();
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn test_refresh_response_serialization() {
        let resp = RefreshResponse {
            status: "ok".into(),
            fetched: 10,
            validated: 5,
            stored: 4,
            errors: 1,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"fetched\":10"));
        assert!(json.contains("\"errors\":1"));
    }

    #[test]
    fn test_simple_response_serialization() {
        let resp = SimpleResponse {
            status: "ok".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
    }

    #[test]
    fn test_status_response_serialization() {
        let resp = StatusResponse {
            version: "0.1.0",
            git_hash: "abc1234",
            pool: PoolStatus {
                http: 10,
                https: 5,
                socks5: 3,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"http\":10"));
        assert!(json.contains("\"git_hash\":\"abc1234\""));
    }
}
