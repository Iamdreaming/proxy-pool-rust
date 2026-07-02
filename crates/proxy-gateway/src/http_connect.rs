//! HTTP CONNECT proxy handler.

use crate::upstream::{Upstream, UpstreamSelector, connect_via_socks5, connect_via_warp_chain};
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
    let upstream = selector.select(host, "http").await;

    match upstream {
        Upstream::Direct => {
            // Connect directly to target
            match TcpStream::connect(&target).await {
                Ok(mut remote) => {
                    let resp = "HTTP/1.1 200 Connection Established\r\n\r\n";
                    stream.write_all(resp.as_bytes()).await?;
                    bidirectional_copy(stream, &mut remote).await;
                }
                Err(e) => {
                    tracing::warn!("HTTP CONNECT: cannot connect to {target}: {e}");
                    let resp = "HTTP/1.1 502 Bad Gateway\r\n\r\n";
                    stream.write_all(resp.as_bytes()).await?;
                }
            }
        }
        Upstream::Proxy(proxy) => {
            let upstream_addr = format!("{}:{}", proxy.host, proxy.port);
            match connect_via_socks5(&upstream_addr, &target).await {
                Ok(mut remote) => {
                    let resp = "HTTP/1.1 200 Connection Established\r\n\r\n";
                    stream.write_all(resp.as_bytes()).await?;
                    bidirectional_copy(stream, &mut remote).await;
                }
                Err(e) => {
                    tracing::warn!("HTTP CONNECT: SOCKS5 via {} failed: {e}", upstream_addr);
                    let resp = "HTTP/1.1 502 Bad Gateway\r\n\r\n";
                    stream.write_all(resp.as_bytes()).await?;
                }
            }
        }
        Upstream::Warp { socks5_port }
        | Upstream::Xray {
            local_socks5_port: socks5_port,
        } => {
            let upstream_addr = format!("127.0.0.1:{socks5_port}");
            match connect_via_socks5(&upstream_addr, &target).await {
                Ok(mut remote) => {
                    let resp = "HTTP/1.1 200 Connection Established\r\n\r\n";
                    stream.write_all(resp.as_bytes()).await?;
                    bidirectional_copy(stream, &mut remote).await;
                }
                Err(e) => {
                    tracing::warn!("HTTP CONNECT: SOCKS5 via {upstream_addr} failed: {e}");
                    let resp = "HTTP/1.1 502 Bad Gateway\r\n\r\n";
                    stream.write_all(resp.as_bytes()).await?;
                }
            }
        }
        Upstream::WarpChain { proxy, socks5_port } => {
            match connect_via_warp_chain(&proxy, socks5_port, &target).await {
                Ok(mut remote) => {
                    let resp = "HTTP/1.1 200 Connection Established\r\n\r\n";
                    stream.write_all(resp.as_bytes()).await?;
                    bidirectional_copy(stream, &mut remote).await;
                }
                Err(e) => {
                    tracing::warn!(
                        "HTTP CONNECT: WarpChain via {}->WARP:{} failed: {e}",
                        proxy.host,
                        socks5_port
                    );
                    let resp = "HTTP/1.1 502 Bad Gateway\r\n\r\n";
                    stream.write_all(resp.as_bytes()).await?;
                }
            }
        }
        Upstream::NoProxy => {
            let resp = "HTTP/1.1 502 Bad Gateway\r\n\r\n";
            stream.write_all(resp.as_bytes()).await?;
        }
    }

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
