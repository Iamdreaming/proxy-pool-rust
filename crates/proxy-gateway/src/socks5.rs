//! SOCKS5 proxy handler (RFC 1928).

use crate::upstream::{Upstream, UpstreamSelector, connect_via_socks5, connect_via_warp_chain};
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
    let upstream = selector.select(host, "socks5").await;

    match upstream {
        Upstream::Direct => match TcpStream::connect(&target_addr).await {
            Ok(mut remote) => {
                let local_addr = remote.local_addr().unwrap_or("0.0.0.0:0".parse().unwrap());
                let reply = socks5_reply_from_addr(0x00, &local_addr);
                stream.write_all(&reply).await?;
                bidirectional_copy(stream, &mut remote).await;
            }
            Err(e) => {
                tracing::warn!("SOCKS5: cannot connect to {target_addr}: {e}");
                let reply = socks5_reply(0x05, "0.0.0.0:0"); // connection refused
                stream.write_all(&reply).await?;
            }
        },
        Upstream::Proxy(proxy) => {
            let upstream_addr = format!("{}:{}", proxy.host, proxy.port);
            match connect_via_socks5(&upstream_addr, &target_addr).await {
                Ok(mut remote) => {
                    let local_addr = remote.local_addr().unwrap_or("0.0.0.0:0".parse().unwrap());
                    let reply = socks5_reply_from_addr(0x00, &local_addr);
                    stream.write_all(&reply).await?;
                    bidirectional_copy(stream, &mut remote).await;
                }
                Err(e) => {
                    tracing::warn!("SOCKS5: SOCKS5 chain via {} failed: {e}", upstream_addr);
                    let reply = socks5_reply(0x05, "0.0.0.0:0");
                    stream.write_all(&reply).await?;
                }
            }
        }
        Upstream::Warp { socks5_port }
        | Upstream::Xray {
            local_socks5_port: socks5_port,
        } => {
            let upstream_addr = format!("127.0.0.1:{socks5_port}");
            match connect_via_socks5(&upstream_addr, &target_addr).await {
                Ok(mut remote) => {
                    let local_addr = remote.local_addr().unwrap_or("0.0.0.0:0".parse().unwrap());
                    let reply = socks5_reply_from_addr(0x00, &local_addr);
                    stream.write_all(&reply).await?;
                    bidirectional_copy(stream, &mut remote).await;
                }
                Err(e) => {
                    tracing::warn!("SOCKS5: SOCKS5 chain via {upstream_addr} failed: {e}");
                    let reply = socks5_reply(0x05, "0.0.0.0:0");
                    stream.write_all(&reply).await?;
                }
            }
        }
        Upstream::WarpChain { proxy, socks5_port } => {
            match connect_via_warp_chain(&proxy, socks5_port, &target_addr).await {
                Ok(mut remote) => {
                    let local_addr = remote.local_addr().unwrap_or("0.0.0.0:0".parse().unwrap());
                    let reply = socks5_reply_from_addr(0x00, &local_addr);
                    stream.write_all(&reply).await?;
                    bidirectional_copy(stream, &mut remote).await;
                }
                Err(e) => {
                    tracing::warn!(
                        "SOCKS5: WarpChain via {}->WARP:{} failed: {e}",
                        proxy.host,
                        socks5_port
                    );
                    let reply = socks5_reply(0x05, "0.0.0.0:0");
                    stream.write_all(&reply).await?;
                }
            }
        }
        Upstream::NoProxy => {
            let reply = socks5_reply(0x05, "0.0.0.0:0"); // connection refused
            stream.write_all(&reply).await?;
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
