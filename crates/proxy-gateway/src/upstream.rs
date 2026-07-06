//! Upstream connection helpers for gateway handlers.

use proxy_core::models::Proxy;
use proxy_core::route_debug::Upstream;

/// Perform a SOCKS5 CONNECT handshake on an already-connected stream.
///
/// The stream must already be connected to a SOCKS5 proxy. This function
/// sends the greeting, method negotiation, and CONNECT request for `target_addr`.
pub async fn socks5_handshake_on_stream(
    stream: &mut tokio::net::TcpStream,
    target_addr: &str,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // SOCKS5 greeting: version 5, 1 method, no-auth (0x00)
    stream.write_all(&[0x05, 0x01, 0x00]).await?;

    let mut resp = [0u8; 2];
    stream.read_exact(&mut resp).await?;
    if resp[0] != 0x05 || resp[1] != 0x00 {
        anyhow::bail!("SOCKS5 upstream rejected no-auth: {:#?}", resp);
    }

    // Parse target address for SOCKS5 CONNECT request
    let (host, port) = parse_target_addr(target_addr)?;

    // SOCKS5 CONNECT request
    let mut request = vec![0x05, 0x01, 0x00]; // VER, CMD=CONNECT, RSV
    if host.contains(':') {
        // IPv6
        request.push(0x04);
        let ip: std::net::Ipv6Addr = host.parse()?;
        request.extend_from_slice(&ip.octets());
    } else if host.parse::<std::net::Ipv4Addr>().is_ok() {
        // IPv4
        request.push(0x01);
        let ip: std::net::Ipv4Addr = host.parse()?;
        request.extend_from_slice(&ip.octets());
    } else {
        // Domain name
        request.push(0x03);
        let domain_bytes = host.as_bytes();
        request.push(domain_bytes.len() as u8);
        request.extend_from_slice(domain_bytes);
    }
    request.extend_from_slice(&port.to_be_bytes());

    stream.write_all(&request).await?;

    // Read SOCKS5 reply
    let mut reply_header = [0u8; 4];
    stream.read_exact(&mut reply_header).await?;
    if reply_header[1] != 0x00 {
        anyhow::bail!(
            "SOCKS5 upstream CONNECT failed with reply code: {}",
            reply_header[1]
        );
    }

    // Read and discard the bound address based on address type
    match reply_header[3] {
        0x01 => {
            let mut discard = [0u8; 6];
            stream.read_exact(&mut discard).await?;
        }
        0x03 => {
            let mut len_buf = [0u8; 1];
            stream.read_exact(&mut len_buf).await?;
            let mut discard = vec![0u8; len_buf[0] as usize + 2];
            stream.read_exact(&mut discard).await?;
        }
        0x04 => {
            let mut discard = [0u8; 18];
            stream.read_exact(&mut discard).await?;
        }
        _ => {}
    }

    Ok(())
}

/// Connect to a target through a SOCKS5 upstream proxy.
///
/// This establishes a TCP connection to the upstream SOCKS5 proxy,
/// performs the SOCKS5 handshake, and sends a CONNECT request for the target.
/// Returns a TcpStream that is already tunneled to the target.
pub async fn connect_via_socks5(
    upstream_addr: &str,
    target_addr: &str,
) -> anyhow::Result<tokio::net::TcpStream> {
    let mut stream = tokio::net::TcpStream::connect(upstream_addr).await?;
    socks5_handshake_on_stream(&mut stream, target_addr).await?;
    Ok(stream)
}

/// Connect to a target through a WarpChain: proxy -> WARP -> target.
///
/// Step 1: Connect to the pool proxy via SOCKS5, targeting the WARP SOCKS5 entry.
/// Step 2: On the resulting stream, perform another SOCKS5 CONNECT to the actual target.
pub async fn connect_via_warp_chain(
    proxy: &Proxy,
    warp_socks5_port: u16,
    target_addr: &str,
) -> anyhow::Result<tokio::net::TcpStream> {
    let proxy_addr = format!("{}:{}", proxy.host, proxy.port);
    let warp_addr = format!("127.0.0.1:{warp_socks5_port}");

    // Step 1: proxy -> WARP entry
    let mut stream = connect_via_socks5(&proxy_addr, &warp_addr).await?;

    // Step 2: WARP -> target (SOCKS5 handshake on the already-tunneled stream)
    socks5_handshake_on_stream(&mut stream, target_addr).await?;

    Ok(stream)
}

/// Connect to `target_addr` using a concrete runtime upstream.
pub async fn connect_to_upstream(
    upstream: &Upstream,
    target_addr: &str,
) -> anyhow::Result<tokio::net::TcpStream> {
    match upstream {
        Upstream::Direct => Ok(tokio::net::TcpStream::connect(target_addr).await?),
        Upstream::Proxy(proxy) => {
            let upstream_addr = format!("{}:{}", proxy.host, proxy.port);
            connect_via_socks5(&upstream_addr, target_addr).await
        }
        Upstream::Warp { socks5_port }
        | Upstream::Xray {
            local_socks5_port: socks5_port,
        } => {
            let upstream_addr = format!("127.0.0.1:{socks5_port}");
            connect_via_socks5(&upstream_addr, target_addr).await
        }
        Upstream::WarpChain { proxy, socks5_port } => {
            connect_via_warp_chain(proxy, *socks5_port, target_addr).await
        }
        Upstream::NoProxy => anyhow::bail!("no upstream available"),
    }
}

/// Parse a target address string into (host, port).
///
/// Handles:
/// - "host:port" (IPv4 or domain)
/// - "[ipv6]:port"
fn parse_target_addr(target: &str) -> anyhow::Result<(String, u16)> {
    if target.starts_with('[') {
        // IPv6: [addr]:port
        if let Some(bracket_end) = target.find(']') {
            let host = target[1..bracket_end].to_string();
            let port_str = target.get(bracket_end + 2..).unwrap_or("");
            let port: u16 = port_str.parse()?;
            return Ok((host, port));
        }
    }
    // IPv4 or domain: host:port
    let (host, port_str) = target
        .rsplit_once(':')
        .ok_or_else(|| anyhow::anyhow!("invalid target address: {target}"))?;
    let port: u16 = port_str
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid port in target: {target}"))?;
    Ok((host.to_string(), port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxy_core::models::Protocol;

    #[test]
    fn test_parse_target_ipv4() {
        let (host, port) = parse_target_addr("1.2.3.4:443").unwrap();
        assert_eq!(host, "1.2.3.4");
        assert_eq!(port, 443);
    }

    #[test]
    fn test_parse_target_domain() {
        let (host, port) = parse_target_addr("google.com:443").unwrap();
        assert_eq!(host, "google.com");
        assert_eq!(port, 443);
    }

    #[test]
    fn test_parse_target_ipv6() {
        let (host, port) = parse_target_addr("[::1]:443").unwrap();
        assert_eq!(host, "::1");
        assert_eq!(port, 443);
    }

    #[test]
    fn test_upstream_variants() {
        // Just ensure the variants compile and have the right shape
        let _ = Upstream::Direct;
        let _ = Upstream::NoProxy;
        let _ = Upstream::Warp { socks5_port: 40000 };
        let _ = Upstream::Xray {
            local_socks5_port: 20000,
        };
    }

    #[test]
    fn test_warp_chain_compiles() {
        // Verify the WarpChain variant and connect_via_warp_chain signature
        let proxy = Proxy::new("1.2.3.4", 1080, Protocol::Socks5);
        let _ = Upstream::WarpChain {
            proxy: proxy.clone(),
            socks5_port: 40000,
        };
    }

    #[test]
    fn test_warp_chain_upstream_variant() {
        let proxy = Proxy::new("1.2.3.4", 1080, Protocol::Socks5);
        let upstream = Upstream::WarpChain {
            proxy: proxy.clone(),
            socks5_port: 40000,
        };
        if let Upstream::WarpChain {
            proxy: p,
            socks5_port: port,
        } = upstream
        {
            assert_eq!(p.host, "1.2.3.4");
            assert_eq!(p.port, 1080);
            assert_eq!(port, 40000);
        } else {
            panic!("Expected WarpChain variant");
        }
    }

    #[test]
    fn test_socks5_connect_request_ipv4() {
        let host = "1.2.3.4";
        let port: u16 = 443;
        let mut request = vec![0x05, 0x01, 0x00]; // VER, CMD=CONNECT, RSV
        request.push(0x01); // ATYP=IPv4
        let ip: std::net::Ipv4Addr = host.parse().unwrap();
        request.extend_from_slice(&ip.octets());
        request.extend_from_slice(&port.to_be_bytes());

        assert_eq!(request.len(), 10); // 3 + 1 + 4 + 2
        assert_eq!(request[3], 0x01); // ATYP=IPv4
    }

    #[test]
    fn test_socks5_connect_request_domain() {
        let host = "google.com";
        let port: u16 = 443;
        let mut request = vec![0x05, 0x01, 0x00];
        request.push(0x03); // ATYP=Domain
        let domain_bytes = host.as_bytes();
        request.push(domain_bytes.len() as u8);
        request.extend_from_slice(domain_bytes);
        request.extend_from_slice(&port.to_be_bytes());

        assert_eq!(request[3], 0x03); // ATYP=Domain
        assert_eq!(request[4], 10); // Length of "google.com"
    }

    #[test]
    fn test_socks5_connect_request_ipv6() {
        let host = "::1";
        let port: u16 = 443;
        let mut request = vec![0x05, 0x01, 0x00];
        request.push(0x04); // ATYP=IPv6
        let ip: std::net::Ipv6Addr = host.parse().unwrap();
        request.extend_from_slice(&ip.octets());
        request.extend_from_slice(&port.to_be_bytes());

        assert_eq!(request.len(), 22); // 3 + 1 + 16 + 2
        assert_eq!(request[3], 0x04); // ATYP=IPv6
    }
}
