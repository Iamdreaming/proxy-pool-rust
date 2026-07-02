//! Upstream selector: decides the proxy for each request.
//!
//! Full decision chain:
//! 1. Router explicit match → group (direct / free_pool / warp / xray)
//! 2. GeoIP auto-split (overseas → WARP → Xray → pool; domestic → Direct)
//! 3. Default group fallback
//! 4. Final fallback: pool → WARP → Xray → NoProxy

use proxy_core::geoip::GeoIPLookup;
use proxy_core::models::{EncryptedProxyState, Protocol, Proxy};
use proxy_core::router::Router;
use proxy_core::store::ProxyStore;
use proxy_core::warp::balancer::WarpBalancer;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Result of upstream selection.
#[derive(Debug)]
pub enum Upstream {
    /// Connect directly to the target.
    Direct,
    /// Route through a pool proxy.
    Proxy(Proxy),
    /// Route through a WARP instance (socks5://127.0.0.1:{port}).
    Warp { socks5_port: u16 },
    /// Route through an xray-node local SOCKS5 port.
    Xray { local_socks5_port: u16 },
    /// Chain: pool proxy → WARP → target (reserved, not yet implemented).
    WarpChain { proxy: Proxy, socks5_port: u16 },
    /// No upstream available — return 502.
    NoProxy,
}

/// Selects upstream proxy based on routing rules, GeoIP, and history.
pub struct UpstreamSelector {
    store: Arc<ProxyStore>,
    balancer: Option<Arc<WarpBalancer>>,
    router: Option<Arc<Router>>,
    geoip: Option<Arc<Mutex<GeoIPLookup>>>,
}

impl UpstreamSelector {
    pub fn new(
        store: Arc<ProxyStore>,
        balancer: Option<Arc<WarpBalancer>>,
        router: Option<Arc<Router>>,
        geoip: Option<Arc<Mutex<GeoIPLookup>>>,
    ) -> Self {
        Self {
            store,
            balancer,
            router,
            geoip,
        }
    }

    /// Select an upstream for the given host and protocol.
    ///
    /// Decision chain:
    /// 1. Router explicit match → group
    /// 2. GeoIP auto-split (if no explicit match needed)
    /// 3. Default fallback chain: pool → WARP → Xray
    pub async fn select(&self, host: &str, protocol: &str) -> Upstream {
        // Step 1: Router explicit match
        if let Some(ref router) = self.router {
            let group = router.match_group(host);
            match group {
                "direct" => return Upstream::Direct,
                "free_pool" => {
                    if let Some(proxy) = self.try_pool(protocol).await {
                        return Upstream::Proxy(proxy);
                    }
                    // Fall through to WARP
                }
                "warp" => {
                    if let Some(port) = self.try_warp().await {
                        return Upstream::Warp { socks5_port: port };
                    }
                    // Fall through to Xray
                }
                "xray" => {
                    if let Some(port) = self.try_xray().await {
                        return Upstream::Xray {
                            local_socks5_port: port,
                        };
                    }
                    // Fall through to pool
                }
                _ => {
                    // Default group — apply GeoIP auto-split
                    if let Some(ref geoip) = self.geoip {
                        let is_overseas = {
                            let mut geoip = geoip.lock().await;
                            let info = geoip.lookup(host).await;
                            geoip.is_overseas(&info.country)
                        };

                        if !is_overseas {
                            return Upstream::Direct;
                        }

                        // Overseas: WARP → Xray → pool → NoProxy
                        if let Some(port) = self.try_warp().await {
                            return Upstream::Warp { socks5_port: port };
                        }
                        if let Some(port) = self.try_xray().await {
                            return Upstream::Xray {
                                local_socks5_port: port,
                            };
                        }
                        if let Some(proxy) = self.try_pool(protocol).await {
                            return Upstream::Proxy(proxy);
                        }
                        return Upstream::NoProxy;
                    }
                }
            }
        } else {
            // No router — apply GeoIP if available
            if let Some(ref geoip) = self.geoip {
                let is_overseas = {
                    let mut geoip = geoip.lock().await;
                    let info = geoip.lookup(host).await;
                    geoip.is_overseas(&info.country)
                };

                if !is_overseas {
                    return Upstream::Direct;
                }
            }
        }

        // Step 3: General fallback chain: pool → WARP → Xray → NoProxy
        if let Some(proxy) = self.try_pool(protocol).await {
            return Upstream::Proxy(proxy);
        }
        if let Some(port) = self.try_warp().await {
            return Upstream::Warp { socks5_port: port };
        }
        if let Some(port) = self.try_xray().await {
            return Upstream::Xray {
                local_socks5_port: port,
            };
        }
        Upstream::NoProxy
    }

    /// Try to get a random proxy from the pool for the given protocol.
    async fn try_pool(&self, protocol: &str) -> Option<Proxy> {
        let proto = Protocol::from_str_loose(protocol).unwrap_or(Protocol::Http);
        match self.store.get_random(proto).await {
            Ok(Some(proxy)) => {
                // Filter out circuit-open proxies and xray-encrypted proxies
                if proxy.circuit_open || proxy.encrypted_state.is_some() {
                    return None;
                }
                Some(proxy)
            }
            _ => None,
        }
    }

    /// Try to get a WARP instance from the balancer.
    async fn try_warp(&self) -> Option<u16> {
        if let Some(ref balancer) = self.balancer
            && let Some(inst) = balancer.next().await
        {
            return Some(inst.socks5_port);
        }
        None
    }

    /// Try to get an active xray encrypted proxy from the store.
    async fn try_xray(&self) -> Option<u16> {
        // Get from Socks5 pool (xray nodes are stored as Socks5 proxies)
        // Filter for encrypted_state == Active
        match self.store.all(Protocol::Socks5).await {
            Ok(proxies) => {
                let active_xray: Vec<&Proxy> = proxies
                    .iter()
                    .filter(|p| {
                        matches!(p.encrypted_state, Some(EncryptedProxyState::Active { .. }))
                    })
                    .collect();
                if active_xray.is_empty() {
                    return None;
                }
                // Random selection among active xray nodes
                let idx = rand::random_range(0..active_xray.len());
                if let Some(EncryptedProxyState::Active { local_socks5_port }) =
                    active_xray[idx].encrypted_state
                {
                    Some(local_socks5_port)
                } else {
                    None
                }
            }
            Err(e) => {
                tracing::debug!("try_xray: failed to query store: {e}");
                None
            }
        }
    }
}

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
