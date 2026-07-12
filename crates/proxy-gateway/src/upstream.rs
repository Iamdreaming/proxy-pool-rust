//! Upstream connection helpers for gateway handlers.

use proxy_core::models::{Protocol, Proxy};
use proxy_core::route_debug::Upstream;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn connect_to_upstream_with_timeout(
    upstream: &Upstream,
    target_addr: &str,
    timeout: Duration,
) -> anyhow::Result<tokio::net::TcpStream> {
    match tokio::time::timeout(timeout, connect_to_upstream(upstream, target_addr)).await {
        Ok(result) => result,
        Err(_) => anyhow::bail!("upstream connect timed out after {}ms", timeout.as_millis()),
    }
}

/// Perform a SOCKS5 CONNECT handshake on an already-connected stream.
///
/// The stream must already be connected to a SOCKS5 proxy. This function
/// sends the greeting, method negotiation, and CONNECT request for `target_addr`.
pub async fn socks5_handshake_on_stream(
    stream: &mut tokio::net::TcpStream,
    target_addr: &str,
    credentials: Option<(&str, &str)>,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // SOCKS5 greeting: offer no-auth (0x00), plus username/password (0x02) when
    // credentials are available.
    if credentials.is_some() {
        stream.write_all(&[0x05, 0x02, 0x00, 0x02]).await?;
    } else {
        stream.write_all(&[0x05, 0x01, 0x00]).await?;
    }

    let mut resp = [0u8; 2];
    stream.read_exact(&mut resp).await?;
    if resp[0] != 0x05 {
        anyhow::bail!("SOCKS5 upstream bad version: {:#?}", resp);
    }
    match resp[1] {
        0x00 => {} // no auth
        0x02 => {
            let (user, pass) = credentials.ok_or_else(|| {
                anyhow::anyhow!("SOCKS5 upstream requires auth but none provided")
            })?;
            if user.len() > 255 || pass.len() > 255 {
                anyhow::bail!("SOCKS5 username/password too long for RFC1929");
            }
            // RFC 1929: VER(0x01), ULEN, UNAME, PLEN, PASSWD
            let mut auth = vec![0x01, user.len() as u8];
            auth.extend_from_slice(user.as_bytes());
            auth.push(pass.len() as u8);
            auth.extend_from_slice(pass.as_bytes());
            stream.write_all(&auth).await?;
            let mut auth_resp = [0u8; 2];
            stream.read_exact(&mut auth_resp).await?;
            if auth_resp[0] != 0x01 || auth_resp[1] != 0x00 {
                anyhow::bail!("SOCKS5 username/password auth rejected");
            }
        }
        other => anyhow::bail!("SOCKS5 upstream selected unsupported method: {other}"),
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
    credentials: Option<(&str, &str)>,
) -> anyhow::Result<tokio::net::TcpStream> {
    let mut stream = tokio::net::TcpStream::connect(upstream_addr).await?;
    socks5_handshake_on_stream(&mut stream, target_addr, credentials).await?;
    Ok(stream)
}

/// Connect to a target through an HTTP proxy using CONNECT.
///
/// This establishes a TCP connection to the upstream HTTP proxy, sends a
/// CONNECT request for `target_addr` (with `Proxy-Authorization` when
/// credentials are supplied), and returns a stream tunneled to the target when
/// the proxy replies with any 2xx status.
pub async fn connect_via_http_proxy(
    upstream_addr: &str,
    target_addr: &str,
    credentials: Option<(&str, &str)>,
) -> anyhow::Result<tokio::net::TcpStream> {
    let mut stream = tokio::net::TcpStream::connect(upstream_addr).await?;
    let auth_header = match credentials {
        Some((user, pass)) => {
            use base64::Engine;
            let token = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
            format!("Proxy-Authorization: Basic {token}\r\n")
        }
        None => String::new(),
    };
    let request = format!(
        "CONNECT {target_addr} HTTP/1.1\r\nHost: {target_addr}\r\n{auth_header}Proxy-Connection: keep-alive\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte).await?;
        if n == 0 {
            anyhow::bail!("HTTP proxy closed before CONNECT response completed");
        }
        response.push(byte[0]);
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
        if response.len() > 8192 {
            anyhow::bail!("HTTP proxy CONNECT response headers too large");
        }
    }

    let response_text = String::from_utf8_lossy(&response);
    let status_line = response_text.lines().next().unwrap_or("");
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| anyhow::anyhow!("invalid HTTP proxy CONNECT response: {status_line}"))?;
    if !(200..300).contains(&status_code) {
        anyhow::bail!("HTTP proxy CONNECT failed with status: {status_code}");
    }

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

    // Step 1: proxy -> WARP entry (authenticate to the pool proxy if it needs it)
    let mut stream = connect_via_socks5(&proxy_addr, &warp_addr, proxy.credentials()).await?;

    // Step 2: WARP -> target (SOCKS5 handshake on the already-tunneled stream)
    socks5_handshake_on_stream(&mut stream, target_addr, None).await?;

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
            let creds = proxy.credentials();
            match proxy.protocol {
                Protocol::Http | Protocol::Https => {
                    connect_via_http_proxy(&upstream_addr, target_addr, creds).await
                }
                Protocol::Socks5 => connect_via_socks5(&upstream_addr, target_addr, creds).await,
                Protocol::Socks4 => anyhow::bail!("SOCKS4 upstream proxies are not supported"),
            }
        }
        Upstream::Warp { socks5_port, .. } => {
            let upstream_addr = format!("127.0.0.1:{socks5_port}");
            connect_via_socks5(&upstream_addr, target_addr, None).await
        }
        Upstream::Xray {
            local_socks5_port: socks5_port,
        } => {
            let upstream_addr = format!("127.0.0.1:{socks5_port}");
            connect_via_socks5(&upstream_addr, target_addr, None).await
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
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    async fn read_http_headers(stream: &mut TcpStream) -> Vec<u8> {
        let mut response = Vec::new();
        let mut buf = [0u8; 128];
        loop {
            let n = stream.read(&mut buf).await.unwrap();
            assert!(n > 0, "stream closed before HTTP headers completed");
            response.extend_from_slice(&buf[..n]);
            if response.windows(4).any(|window| window == b"\r\n\r\n") {
                return response;
            }
            assert!(
                response.len() <= 8192,
                "HTTP headers exceeded test size limit"
            );
        }
    }

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
        let _ = Upstream::Warp {
            id: 1,
            socks5_port: 40000,
        };
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

    #[tokio::test]
    async fn test_http_proxy_upstream_uses_http_connect() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_http_headers(&mut socket).await;
            let request_text = String::from_utf8(request).unwrap();
            assert!(request_text.starts_with("CONNECT example.com:443 HTTP/1.1\r\n"));
            assert!(request_text.contains("\r\nHost: example.com:443\r\n"));

            socket
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .unwrap();
            let mut tunneled = [0u8; 4];
            socket.read_exact(&mut tunneled).await.unwrap();
            assert_eq!(&tunneled, b"ping");
            socket.write_all(b"pong").await.unwrap();
        });

        let proxy = Proxy::new("127.0.0.1", upstream_port, Protocol::Http);
        let mut stream = connect_to_upstream(&Upstream::Proxy(proxy), "example.com:443")
            .await
            .unwrap();
        stream.write_all(b"ping").await.unwrap();
        let mut response = [0u8; 4];
        stream.read_exact(&mut response).await.unwrap();
        assert_eq!(&response, b"pong");

        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_http_proxy_connect_preserves_tunneled_bytes_after_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _request = read_http_headers(&mut socket).await;
            socket
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\npreface")
                .await
                .unwrap();
        });

        let mut stream = connect_via_http_proxy(
            &format!("127.0.0.1:{upstream_port}"),
            "example.com:443",
            None,
        )
        .await
        .unwrap();
        let mut tunneled = [0u8; 7];
        stream.read_exact(&mut tunneled).await.unwrap();
        assert_eq!(&tunneled, b"preface");

        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_socks5_proxy_upstream_uses_socks5_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut greeting = [0u8; 3];
            socket.read_exact(&mut greeting).await.unwrap();
            assert_eq!(greeting, [0x05, 0x01, 0x00]);
            socket.write_all(&[0x05, 0x00]).await.unwrap();

            let mut header = [0u8; 4];
            socket.read_exact(&mut header).await.unwrap();
            assert_eq!(header, [0x05, 0x01, 0x00, 0x03]);

            let mut domain_len = [0u8; 1];
            socket.read_exact(&mut domain_len).await.unwrap();
            let mut domain = vec![0u8; domain_len[0] as usize];
            socket.read_exact(&mut domain).await.unwrap();
            assert_eq!(domain, b"example.com");

            let mut port = [0u8; 2];
            socket.read_exact(&mut port).await.unwrap();
            assert_eq!(u16::from_be_bytes(port), 443);

            socket
                .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0])
                .await
                .unwrap();
            let mut tunneled = [0u8; 4];
            socket.read_exact(&mut tunneled).await.unwrap();
            assert_eq!(&tunneled, b"ping");
            socket.write_all(b"pong").await.unwrap();
        });

        let proxy = Proxy::new("127.0.0.1", upstream_port, Protocol::Socks5);
        let mut stream = connect_to_upstream(&Upstream::Proxy(proxy), "example.com:443")
            .await
            .unwrap();
        stream.write_all(b"ping").await.unwrap();
        let mut response = [0u8; 4];
        stream.read_exact(&mut response).await.unwrap();
        assert_eq!(&response, b"pong");

        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_upstream_attempt_timeout_bounds_slow_proxy() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;
        });

        let proxy = Proxy::new("127.0.0.1", upstream_port, Protocol::Http);
        let err = connect_to_upstream_with_timeout(
            &Upstream::Proxy(proxy),
            "example.com:443",
            Duration::from_millis(50),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("timed out"));

        server.abort();
    }

    #[tokio::test]
    async fn test_socks4_proxy_upstream_is_rejected_without_connecting() {
        let proxy = Proxy::new("127.0.0.1", 9, Protocol::Socks4);
        let err = connect_to_upstream(&Upstream::Proxy(proxy), "example.com:80")
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("SOCKS4 upstream proxies are not supported")
        );
    }

    #[tokio::test]
    async fn test_http_proxy_connect_sends_proxy_authorization() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_http_headers(&mut socket).await;
            let request_text = String::from_utf8(request).unwrap();
            assert!(request_text.starts_with("CONNECT example.com:443 HTTP/1.1\r\n"));
            // base64("user:pass") = dXNlcjpwYXNz
            assert!(request_text.contains("\r\nProxy-Authorization: Basic dXNlcjpwYXNz\r\n"));
            socket
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .unwrap();
        });

        connect_via_http_proxy(
            &format!("127.0.0.1:{upstream_port}"),
            "example.com:443",
            Some(("user", "pass")),
        )
        .await
        .unwrap();

        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_socks5_proxy_upstream_uses_username_password_auth() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();

            // Greeting: VER, NMETHODS=2, METHODS={0x00, 0x02}
            let mut greeting = [0u8; 4];
            socket.read_exact(&mut greeting).await.unwrap();
            assert_eq!(greeting, [0x05, 0x02, 0x00, 0x02]);
            // Select username/password
            socket.write_all(&[0x05, 0x02]).await.unwrap();

            // RFC 1929 sub-negotiation: VER=0x01, ULEN, UNAME, PLEN, PASSWD
            let mut ver_ulen = [0u8; 2];
            socket.read_exact(&mut ver_ulen).await.unwrap();
            assert_eq!(ver_ulen, [0x01, 4]); // "user".len()
            let mut uname = [0u8; 4];
            socket.read_exact(&mut uname).await.unwrap();
            assert_eq!(&uname, b"user");
            let mut plen = [0u8; 1];
            socket.read_exact(&mut plen).await.unwrap();
            assert_eq!(plen, [4]); // "pass".len()
            let mut passwd = [0u8; 4];
            socket.read_exact(&mut passwd).await.unwrap();
            assert_eq!(&passwd, b"pass");
            // Auth success
            socket.write_all(&[0x01, 0x00]).await.unwrap();

            // CONNECT request for example.com:443
            let mut header = [0u8; 4];
            socket.read_exact(&mut header).await.unwrap();
            assert_eq!(header, [0x05, 0x01, 0x00, 0x03]);
            let mut domain_len = [0u8; 1];
            socket.read_exact(&mut domain_len).await.unwrap();
            let mut domain = vec![0u8; domain_len[0] as usize];
            socket.read_exact(&mut domain).await.unwrap();
            assert_eq!(domain, b"example.com");
            let mut port = [0u8; 2];
            socket.read_exact(&mut port).await.unwrap();
            assert_eq!(u16::from_be_bytes(port), 443);

            socket
                .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0])
                .await
                .unwrap();
        });

        let mut proxy = Proxy::new("127.0.0.1", upstream_port, Protocol::Socks5);
        proxy.username = Some("user".into());
        proxy.password = Some("pass".into());
        connect_to_upstream(&Upstream::Proxy(proxy), "example.com:443")
            .await
            .unwrap();

        server.await.unwrap();
    }
}
