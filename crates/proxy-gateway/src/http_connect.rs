//! HTTP CONNECT proxy handler.

use proxy_core::store::ProxyStore;
use proxy_core::warp::balancer::WarpBalancer;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Handle an HTTP CONNECT request.
pub async fn handle(
    mut stream: TcpStream,
    _client_addr: SocketAddr,
    _store: Arc<ProxyStore>,
    _balancer: Option<Arc<WarpBalancer>>,
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

    // TODO: Use UpstreamSelector to choose upstream
    // For now, direct connect
    match TcpStream::connect(&target).await {
        Ok(mut remote) => {
            // Send 200 Connection Established
            let resp = "HTTP/1.1 200 Connection Established\r\n\r\n";
            stream.write_all(resp.as_bytes()).await?;

            // Bidirectional copy
            let (mut ri, mut wi) = stream.split();
            let (mut ro, mut wo) = remote.split();

            let client_to_server = tokio::io::copy(&mut ri, &mut wo);
            let server_to_client = tokio::io::copy(&mut ro, &mut wi);

            tokio::select! {
                r = client_to_server => { if let Err(e) = r { tracing::debug!("client→server error: {e}"); } }
                r = server_to_client => { if let Err(e) = r { tracing::debug!("server→client error: {e}"); } }
            }
        }
        Err(e) => {
            tracing::warn!("cannot connect to {target}: {e}");
            let resp = "HTTP/1.1 502 Bad Gateway\r\n\r\n";
            stream.write_all(resp.as_bytes()).await?;
        }
    }

    Ok(())
}
