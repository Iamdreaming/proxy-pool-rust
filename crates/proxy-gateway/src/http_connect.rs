//! HTTP CONNECT proxy handler.

use crate::upstream::{UPSTREAM_CONNECT_TIMEOUT, connect_to_upstream_with_timeout};
use proxy_core::route_debug::{
    GatewayAttemptStatus, GatewayRouteProtocol, RouteExit, Upstream, UpstreamSelector,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Handle an HTTP CONNECT request.
pub async fn handle(
    mut stream: TcpStream,
    _client_addr: SocketAddr,
    selector: Arc<UpstreamSelector>,
) -> anyhow::Result<()> {
    // Read the CONNECT request line
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse: CONNECT host:port HTTP/1.1
    let first_line = request.lines().next().unwrap_or("");
    if !first_line.starts_with("CONNECT ") {
        // Not a CONNECT request — respond with 400
        let resp = "HTTP/1.1 400 Bad Request\r\n\r\n";
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    let target = first_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .to_string();

    if target.is_empty() {
        let resp = "HTTP/1.1 400 Bad Request\r\n\r\n";
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    let host = target.split(':').next().unwrap_or(&target);
    let selection = selector.select_with_trace(host, "http").await;
    let metrics = selector.metrics();
    for candidate in &selection.upstream_candidates {
        if matches!(candidate.upstream, Upstream::NoProxy) {
            metrics.record(
                GatewayRouteProtocol::HttpConnect,
                RouteExit::NoProxy,
                GatewayAttemptStatus::Unavailable,
            );
            continue;
        }

        match connect_to_upstream_with_timeout(
            &candidate.upstream,
            &target,
            UPSTREAM_CONNECT_TIMEOUT,
        )
        .await
        {
            Ok(mut remote) => {
                metrics.record(
                    GatewayRouteProtocol::HttpConnect,
                    candidate.exit,
                    GatewayAttemptStatus::Success,
                );
                let resp = "HTTP/1.1 200 Connection Established\r\n\r\n";
                stream.write_all(resp.as_bytes()).await?;
                bidirectional_copy(stream, &mut remote).await;
                return Ok(());
            }
            Err(e) => {
                metrics.record(
                    GatewayRouteProtocol::HttpConnect,
                    candidate.exit,
                    GatewayAttemptStatus::Failure,
                );
                tracing::warn!(
                    target = %target,
                    route_group = ?selection.decision.matched_group,
                    exit = ?candidate.exit,
                    detail = ?candidate.detail,
                    error = %e,
                    "HTTP CONNECT: upstream attempt failed"
                );
            }
        }
    }

    let resp = "HTTP/1.1 502 Bad Gateway\r\n\r\n";
    stream.write_all(resp.as_bytes()).await?;
    Ok(())
}

/// Bidirectional copy between client stream and remote stream.
async fn bidirectional_copy(mut stream: TcpStream, remote: &mut TcpStream) {
    let (mut ri, mut wi) = stream.split();
    let (mut ro, mut wo) = remote.split();

    let client_to_server = tokio::io::copy(&mut ri, &mut wo);
    let server_to_client = tokio::io::copy(&mut ro, &mut wi);

    tokio::select! {
        r = client_to_server => { if let Err(e) = r { tracing::debug!("client→server error: {e}"); } }
        r = server_to_client => { if let Err(e) = r { tracing::debug!("server→client error: {e}"); } }
    }
}
