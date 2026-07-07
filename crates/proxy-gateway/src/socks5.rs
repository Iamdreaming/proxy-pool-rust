//! SOCKS5 proxy handler (RFC 1928).

use crate::upstream::{UPSTREAM_CONNECT_TIMEOUT, connect_to_upstream_with_timeout};
use proxy_core::route_debug::{
    GatewayAttemptStatus, GatewayRouteProtocol, RouteExit, Upstream, UpstreamSelector,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Handle a SOCKS5 connection.
pub async fn handle(
    mut stream: TcpStream,
    _client_addr: SocketAddr,
    selector: Arc<UpstreamSelector>,
) -> anyhow::Result<()> {
    // --- Phase 1: Method selection ---
    let mut buf = [0u8; 2];
    stream.read_exact(&mut buf).await?;
    if buf[0] != 0x05 {
        return Err(anyhow::anyhow!("not a SOCKS5 connection"));
    }
    let n_methods = buf[1] as usize;
    let mut methods = vec![0u8; n_methods];
    stream.read_exact(&mut methods).await?;

    // Support no-auth (0x00) only
    if !methods.contains(&0x00) {
        let resp = [0x05, 0xFF]; // no acceptable methods
        stream.write_all(&resp).await?;
        return Err(anyhow::anyhow!("SOCKS5: no acceptable auth method"));
    }

    // Select no-auth
    stream.write_all(&[0x05, 0x00]).await?;

    // --- Phase 2: Request ---
    let mut req_buf = [0u8; 4];
    stream.read_exact(&mut req_buf).await?;

    if req_buf[0] != 0x05 {
        return Err(anyhow::anyhow!("SOCKS5: invalid version in request"));
    }
    let cmd = req_buf[1];
    if cmd != 0x01 {
        // Only CONNECT supported
        let reply = socks5_reply(0x07, "0.0.0.0:0"); // command not supported
        stream.write_all(&reply).await?;
        return Ok(());
    }

    let target_addr = match req_buf[3] {
        // IPv4
        0x01 => {
            let mut ip = [0u8; 4];
            stream.read_exact(&mut ip).await?;
            let mut port_buf = [0u8; 2];
            stream.read_exact(&mut port_buf).await?;
            let port = u16::from_be_bytes(port_buf);
            format!("{}.{}.{}.{}:{}", ip[0], ip[1], ip[2], ip[3], port)
        }
        // Domain
        0x03 => {
            let mut len_buf = [0u8; 1];
            stream.read_exact(&mut len_buf).await?;
            let domain_len = len_buf[0] as usize;
            let mut domain = vec![0u8; domain_len];
            stream.read_exact(&mut domain).await?;
            let mut port_buf = [0u8; 2];
            stream.read_exact(&mut port_buf).await?;
            let port = u16::from_be_bytes(port_buf);
            let domain = String::from_utf8_lossy(&domain).to_string();
            format!("{domain}:{port}")
        }
        // IPv6
        0x04 => {
            let mut ip = [0u8; 16];
            stream.read_exact(&mut ip).await?;
            let mut port_buf = [0u8; 2];
            stream.read_exact(&mut port_buf).await?;
            let port = u16::from_be_bytes(port_buf);
            // Simplified: use numeric IPv6 representation
            let addr: std::net::Ipv6Addr = ip.into();
            format!("[{addr}]:{port}")
        }
        _ => {
            let reply = socks5_reply(0x08, "0.0.0.0:0"); // address type not supported
            stream.write_all(&reply).await?;
            return Ok(());
        }
    };

    // --- Phase 3: Upstream selection ---
    let host = target_addr.split(':').next().unwrap_or(&target_addr);
    let selection = selector.select_with_trace(host, "socks5").await;
    let metrics = selector.metrics();
    for candidate in &selection.upstream_candidates {
        if matches!(candidate.upstream, Upstream::NoProxy) {
            metrics.record(
                GatewayRouteProtocol::Socks5,
                RouteExit::NoProxy,
                GatewayAttemptStatus::Unavailable,
            );
            continue;
        }

        match connect_to_upstream_with_timeout(
            &candidate.upstream,
            &target_addr,
            UPSTREAM_CONNECT_TIMEOUT,
        )
        .await
        {
            Ok(mut remote) => {
                metrics.record(
                    GatewayRouteProtocol::Socks5,
                    candidate.exit,
                    GatewayAttemptStatus::Success,
                );
                selector
                    .record_upstream_attempt(&candidate.upstream, GatewayAttemptStatus::Success)
                    .await;
                let local_addr = remote.local_addr().unwrap_or("0.0.0.0:0".parse().unwrap());
                let reply = socks5_reply_from_addr(0x00, &local_addr);
                stream.write_all(&reply).await?;
                bidirectional_copy(stream, &mut remote).await;
                return Ok(());
            }
            Err(e) => {
                metrics.record(
                    GatewayRouteProtocol::Socks5,
                    candidate.exit,
                    GatewayAttemptStatus::Failure,
                );
                selector
                    .record_upstream_attempt(&candidate.upstream, GatewayAttemptStatus::Failure)
                    .await;
                tracing::warn!(
                    target = %target_addr,
                    route_group = ?selection.decision.matched_group,
                    exit = ?candidate.exit,
                    detail = ?candidate.detail,
                    error = %e,
                    "SOCKS5: upstream attempt failed"
                );
            }
        }
    }

    let reply = socks5_reply(0x05, "0.0.0.0:0");
    stream.write_all(&reply).await?;
    Ok(())
}

/// Bidirectional copy between client stream and remote stream.
async fn bidirectional_copy(mut stream: TcpStream, remote: &mut TcpStream) {
    let (mut ri, mut wi) = stream.split();
    let (mut ro, mut wo) = remote.split();

    let client_to_server = tokio::io::copy(&mut ri, &mut wo);
    let server_to_client = tokio::io::copy(&mut ro, &mut wi);

    tokio::select! {
        r = client_to_server => { if let Err(e) = r { tracing::debug!("SOCKS5 client→server: {e}"); } }
        r = server_to_client => { if let Err(e) = r { tracing::debug!("SOCKS5 server→client: {e}"); } }
    }
}

fn socks5_reply(reply_code: u8, _bind_addr: &str) -> Vec<u8> {
    vec![0x05, reply_code, 0x00, 0x01, 0, 0, 0, 0, 0, 0]
}

fn socks5_reply_from_addr(reply_code: u8, addr: &SocketAddr) -> Vec<u8> {
    let mut buf = vec![0x05, reply_code, 0x00];
    match addr {
        SocketAddr::V4(v4) => {
            buf.push(0x01);
            buf.extend_from_slice(&v4.ip().octets());
            buf.extend_from_slice(&v4.port().to_be_bytes());
        }
        SocketAddr::V6(v6) => {
            buf.push(0x04);
            buf.extend_from_slice(&v6.ip().octets());
            buf.extend_from_slice(&v6.port().to_be_bytes());
        }
    }
    buf
}
