//! API route definitions.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use proxy_core::fetcher::base::FetcherRunReport;
use proxy_core::models::{Protocol, Proxy, ProxyFilter, WarpInstance};
use proxy_core::route_debug::RouteDecision;
use proxy_core::status::{collect_readiness, collect_service_status, render_prometheus_metrics};
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
    // -- filter fields --
    pub country: Option<String>,
    pub anonymity: Option<String>,
    pub max_latency: Option<f64>,
    pub overseas: Option<bool>,
    pub min_score: Option<f64>,
    pub source: Option<String>,
    pub alive: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ProxyFilterQuery {
    pub protocol: Option<String>,
    // -- filter fields --
    pub country: Option<String>,
    pub anonymity: Option<String>,
    pub max_latency: Option<f64>,
    pub overseas: Option<bool>,
    pub min_score: Option<f64>,
    pub source: Option<String>,
    pub alive: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct RouteTestQuery {
    pub host: String,
    pub protocol: Option<String>,
}

// ---------------------------------------------------------------------------
// Query to ProxyFilter conversion
// ---------------------------------------------------------------------------

impl From<&ProxyQuery> for ProxyFilter {
    fn from(q: &ProxyQuery) -> Self {
        ProxyFilter {
            country: q.country.clone(),
            anonymity: q.anonymity.clone(),
            max_latency: q.max_latency,
            overseas: q.overseas,
            min_score: q.min_score,
            source: q.source.clone(),
            alive: q.alive,
        }
    }
}

impl From<&ProxyFilterQuery> for ProxyFilter {
    fn from(q: &ProxyFilterQuery) -> Self {
        ProxyFilter {
            country: q.country.clone(),
            anonymity: q.anonymity.clone(),
            max_latency: q.max_latency,
            overseas: q.overseas,
            min_score: q.min_score,
            source: q.source.clone(),
            alive: q.alive,
        }
    }
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

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
pub struct WarpStatusResponse {
    pub instances: Vec<WarpInstance>,
}

#[derive(Serialize)]
pub struct RefreshResponse {
    pub status: String,
    pub fetched: usize,
    pub validated: usize,
    pub stored: usize,
    pub errors: usize,
    pub fetchers: Vec<FetcherRunReport>,
}

#[derive(Serialize)]
pub struct FetchersResponse {
    pub fetchers: Vec<FetcherRunReport>,
}

#[derive(Serialize)]
pub struct RouteTestResponse {
    pub status: String,
    pub decision: Option<RouteDecision>,
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

pub fn create_router() -> Router<AppState> {
    Router::new()
        .route("/api/healthz", get(healthz))
        .route("/api/readyz", get(readyz))
        .route("/api/status", get(status))
        .route("/api/routes/test", get(route_test))
        .route("/api/fetchers", get(fetcher_status))
        .route("/api/fetchers/{id}/refresh", post(refresh_fetcher))
        .route("/api/proxies", get(list_proxies))
        .route("/api/proxy/random", get(get_random_proxy))
        .route("/api/proxy/best", get(get_best_proxy))
        .route("/api/proxies/refresh", post(refresh_pool))
        .route("/api/proxy/{key}", delete(delete_proxy))
        .route("/api/metrics", get(metrics))
        .route("/api/xray/status", get(xray_status))
        .route("/api/warp", get(warp_status))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a JSON status response with the given HTTP status code.
fn json_status(code: StatusCode, msg: impl Into<String>) -> axum::response::Response {
    (code, Json(SimpleResponse { status: msg.into() })).into_response()
}

fn uptime_sec(state: &AppState) -> u64 {
    state.started_at.elapsed().as_secs()
}

async fn service_status(state: &AppState) -> proxy_core::status::ServiceStatus {
    let xray_active = state.xray_active_count.load(Ordering::Relaxed);
    collect_service_status(
        &state.store,
        state.balancer.as_deref(),
        env!("CARGO_PKG_VERSION"),
        state.git_hash,
        uptime_sec(state),
        xray_active,
    )
    .await
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn healthz() -> impl IntoResponse {
    Json(SimpleResponse {
        status: "ok".into(),
    })
}

async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    let readiness = collect_readiness(&state.store).await;
    if readiness.is_ok() {
        (StatusCode::OK, Json(readiness)).into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(readiness)).into_response()
    }
}

async fn status(State(state): State<AppState>) -> impl IntoResponse {
    Json(service_status(&state).await)
}

async fn route_test(
    State(state): State<AppState>,
    Query(params): Query<RouteTestQuery>,
) -> impl IntoResponse {
    let host = params.host.trim();
    if host.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(RouteTestResponse {
                status: "host is required".into(),
                decision: None,
            }),
        )
            .into_response();
    }

    let protocol = params.protocol.as_deref().unwrap_or("http");
    let decision = state.route_selector.dry_run(host, protocol).await;
    (
        StatusCode::OK,
        Json(RouteTestResponse {
            status: "ok".into(),
            decision: Some(decision),
        }),
    )
        .into_response()
}

async fn list_proxies(
    State(state): State<AppState>,
    Query(params): Query<ProxyQuery>,
) -> impl IntoResponse {
    let protocol_str = params.protocol.as_deref().unwrap_or("http");
    let protocol = Protocol::from_str_loose(protocol_str).unwrap_or(Protocol::Http);
    let limit = params.limit.unwrap_or(100);
    let filter = ProxyFilter::from(&params);

    match state.store.query(protocol, &filter, limit).await {
        Ok(mut proxies) => {
            proxies.truncate(limit);
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
    Query(params): Query<ProxyFilterQuery>,
) -> impl IntoResponse {
    let protocol_str = params.protocol.as_deref().unwrap_or("http");
    let protocol = Protocol::from_str_loose(protocol_str).unwrap_or(Protocol::Http);
    let filter = ProxyFilter::from(&params);

    match state.store.get_random_filtered(protocol, &filter).await {
        Ok(proxy) => Json(proxy),
        Err(e) => {
            tracing::error!("get_random_proxy error: {e}");
            Json(None)
        }
    }
}

async fn get_best_proxy(
    State(state): State<AppState>,
    Query(params): Query<ProxyFilterQuery>,
) -> impl IntoResponse {
    let protocol_str = params.protocol.as_deref().unwrap_or("http");
    let protocol = Protocol::from_str_loose(protocol_str).unwrap_or(Protocol::Http);
    let filter = ProxyFilter::from(&params);

    match state.store.get_best_filtered(protocol, &filter).await {
        Ok(proxy) => Json(proxy),
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
            fetchers: result.fetchers,
        }),
        Err(e) => Json(RefreshResponse {
            status: format!("error: {e}"),
            fetched: 0,
            validated: 0,
            stored: 0,
            errors: 0,
            fetchers: vec![],
        }),
    }
}

async fn fetcher_status(State(state): State<AppState>) -> impl IntoResponse {
    Json(FetchersResponse {
        fetchers: state.scheduler_handle.fetcher_statuses().await,
    })
}

async fn refresh_fetcher(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.scheduler_handle.refresh_fetcher(&id).await {
        Ok(result) => (
            StatusCode::OK,
            Json(RefreshResponse {
                status: "ok".into(),
                fetched: result.fetched,
                validated: result.validated,
                stored: result.stored,
                errors: result.errors,
                fetchers: result.fetchers,
            }),
        )
            .into_response(),
        Err(e) => {
            let code = if e.to_string().contains("fetcher not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (
                code,
                Json(RefreshResponse {
                    status: format!("error: {e}"),
                    fetched: 0,
                    validated: 0,
                    stored: 0,
                    errors: 0,
                    fetchers: vec![],
                }),
            )
                .into_response()
        }
    }
}

async fn delete_proxy(State(state): State<AppState>, Path(key): Path<String>) -> impl IntoResponse {
    let parts: Vec<&str> = key.splitn(3, ':').collect();
    if parts.len() != 3 {
        return json_status(
            StatusCode::BAD_REQUEST,
            "invalid key format, expected protocol:host:port",
        );
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
    let status = service_status(&state).await;
    let mut lines = render_prometheus_metrics(&status);
    lines.push_str(&state.route_selector.render_gateway_metrics());
    ([("content-type", "text/plain")], lines)
}

async fn xray_status(State(state): State<AppState>) -> impl IntoResponse {
    let active = state.xray_active_count.load(Ordering::Relaxed);
    Json(XrayStatusResponse {
        active_nodes: active,
    })
}

async fn warp_status(State(state): State<AppState>) -> impl IntoResponse {
    let instances = match &state.balancer {
        Some(balancer) => balancer.all_list().await,
        None => vec![],
    };
    Json(WarpStatusResponse { instances })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_delete_key_valid() {
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
            fetchers: vec![],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"fetched\":10"));
        assert!(json.contains("\"errors\":1"));
        assert!(json.contains("\"fetchers\""));
    }

    #[test]
    fn test_fetchers_response_serialization() {
        let resp = FetchersResponse { fetchers: vec![] };
        let json = serde_json::to_string(&resp).unwrap();
        assert_eq!(json, "{\"fetchers\":[]}");
    }

    #[test]
    fn test_route_test_response_serialization() {
        let resp = RouteTestResponse {
            status: "ok".into(),
            decision: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"decision\":null"));
    }

    #[test]
    fn test_route_test_query_deserialize() {
        let query: RouteTestQuery =
            serde_json::from_value(serde_json::json!({"host":"example.com","protocol":"socks5"}))
                .unwrap();
        assert_eq!(query.host, "example.com");
        assert_eq!(query.protocol.as_deref(), Some("socks5"));
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
    fn test_readyz_error_serialization() {
        let resp = proxy_core::status::DependencyStatus::error("redis down");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"error\""));
        assert!(json.contains("redis down"));
    }
}
