//! Upstream selector: decides the proxy for each request.

use proxy_core::models::Proxy;
use proxy_core::store::ProxyStore;
use proxy_core::warp::balancer::WarpBalancer;
use std::sync::Arc;

/// Result of upstream selection.
pub enum Upstream {
    /// Connect directly to the target.
    Direct,
    /// Route through a pool proxy.
    Proxy(Proxy),
    /// Route through a WARP instance (socks5://127.0.0.1:{port}).
    Warp { socks5_port: u16 },
    /// No upstream available — return 502.
    NoProxy,
}

/// Selects upstream proxy based on routing rules, GeoIP, and history.
pub struct UpstreamSelector {
    store: Arc<ProxyStore>,
    balancer: Option<Arc<WarpBalancer>>,
}

impl UpstreamSelector {
    pub fn new(store: Arc<ProxyStore>, balancer: Option<Arc<WarpBalancer>>) -> Self {
        Self { store, balancer }
    }

    /// Select an upstream for the given host and protocol.
    pub async fn select(&self, _host: &str, protocol: &str) -> Upstream {
        // TODO: implement full decision chain:
        // 1. Router.match_explicit(host) → group
        // 2. GeoIP auto-split
        // 3. Default group fallback
        // For now, simple pool lookup with WARP fallback
        let proto = proxy_core::models::Protocol::from_str_loose(protocol)
            .unwrap_or(proxy_core::models::Protocol::Http);

        // Try proxy pool first
        if let Ok(Some(proxy)) = self.store.get_random(proto).await {
            return Upstream::Proxy(proxy);
        }

        // Fallback to WARP
        if let Some(balancer) = &self.balancer
            && let Some(inst) = balancer.next().await
        {
            return Upstream::Warp {
                socks5_port: inst.socks5_port,
            };
        }

        Upstream::NoProxy
    }
}
