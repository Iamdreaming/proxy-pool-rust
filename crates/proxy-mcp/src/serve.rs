//! Streamable-HTTP transport server for the MCP service.
//!
//! Extracted so both the (legacy) embedded path and the standalone binary share
//! one implementation.

use crate::ProxyPoolMcp;
use std::sync::Arc;

/// Serve the MCP `ProxyPoolMcp` over the streamable-HTTP transport on `port`.
///
/// Binds `0.0.0.0:{port}` and mounts the MCP service at `/mcp`, with the same
/// OAuth-discovery fallbacks the embedded server used so MCP clients that probe
/// `.well-known` paths get JSON 404s instead of parse errors.
pub async fn serve_http(mcp: ProxyPoolMcp, port: u16) {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };

    let service = StreamableHttpService::new(
        move || Ok(mcp.clone()),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    let app = axum::Router::new()
        .route(
            "/mcp/.well-known/{*path}",
            axum::routing::get(|| async {
                (
                    axum::http::StatusCode::NOT_FOUND,
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    axum::Json(serde_json::json!({
                        "error": "not_found",
                        "error_description": "This server does not require OAuth"
                    })),
                )
            }),
        )
        .nest_service("/mcp", service)
        .fallback(|| async {
            (
                axum::http::StatusCode::NOT_FOUND,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                axum::Json(serde_json::json!({
                    "error": "not_found",
                    "error_description": "This server does not require OAuth"
                })),
            )
        });

    let addr = format!("0.0.0.0:{port}");
    match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => {
            tracing::info!("MCP HTTP transport listening on {addr}");
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!("MCP HTTP transport error: {e}");
            }
        }
        Err(e) => tracing::error!("MCP HTTP bind failed on {addr}: {e}"),
    }
}
