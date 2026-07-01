//! proxy-gateway: SOCKS5/HTTP CONNECT proxy gateway in pure Rust.
//!
//! Replaces the Python mitmproxy-based gateway with a native Rust implementation
//! that supports:
//! - HTTP CONNECT proxying
//! - SOCKS5 proxying
//! - Upstream selection via Router + UpstreamSelector
//! - Smart fallback: free_pool → WARP → 502

mod http_connect;
mod socks5;
mod upstream;

use proxy_core::config::GatewaySettings;
use proxy_core::store::ProxyStore;
use proxy_core::warp::balancer::WarpBalancer;
use std::net::SocketAddr;
use std::sync::Arc;

pub use upstream::UpstreamSelector;

/// The proxy gateway server.
pub struct ProxyGateway {
    settings: GatewaySettings,
    store: Arc<ProxyStore>,
    balancer: Option<Arc<WarpBalancer>>,
}

impl ProxyGateway {
    pub fn new(
        settings: GatewaySettings,
        store: Arc<ProxyStore>,
        balancer: Option<Arc<WarpBalancer>>,
    ) -> Self {
        Self {
            settings,
            store,
            balancer,
        }
    }

    /// Start the gateway server.
    pub async fn run(self: Arc<Self>) -> anyhow::Result<()> {
        let addr = format!(
            "{}:{}",
            self.settings.listen_host, self.settings.listen_port
        );
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        tracing::info!("proxy gateway listening on {addr}");

        loop {
            let (stream, client_addr) = listener.accept().await?;
            let gateway = self.clone();
            tokio::spawn(async move {
                if let Err(e) = gateway.handle_connection(stream, client_addr).await {
                    tracing::debug!("connection error from {client_addr}: {e}");
                }
            });
        }
    }

    /// Detect protocol and dispatch to the appropriate handler.
    async fn handle_connection(
        &self,
        stream: tokio::net::TcpStream,
        client_addr: SocketAddr,
    ) -> anyhow::Result<()> {
        // Peek at the first few bytes to determine protocol
        // HTTP methods start with: CONNECT, GET, POST, PUT, DELETE, etc.
        // SOCKS5 starts with: 0x05
        let mut buf = [0u8; 1];
        let n = stream.peek(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }

        if buf[0] == 0x05 {
            // SOCKS5
            socks5::handle(
                stream,
                client_addr,
                self.store.clone(),
                self.balancer.clone(),
            )
            .await
        } else {
            // HTTP CONNECT
            http_connect::handle(
                stream,
                client_addr,
                self.store.clone(),
                self.balancer.clone(),
            )
            .await
        }
    }
}
