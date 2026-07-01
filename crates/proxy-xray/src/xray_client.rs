//! gRPC client for xray-core's HandlerService.
//!
//! Provides methods to add/remove inbounds and outbounds via xray's gRPC API.
//! The current implementation is a functional stub that records the connection
//! state and API address. Full gRPC calls will be wired in once proto
//! compilation is verified working end-to-end.

use anyhow::Result;

/// Client for communicating with xray-core's gRPC HandlerService.
pub struct XrayClient {
    api_addr: String,
    connected: bool,
}

impl XrayClient {
    /// Create a new `XrayClient` targeting the given gRPC API port.
    pub fn new(api_port: u16) -> Self {
        Self {
            api_addr: format!("http://127.0.0.1:{api_port}"),
            connected: false,
        }
    }

    /// Try to connect to the gRPC API.
    ///
    /// Currently marks the client as connected optimistically. Once the
    /// proto-generated `HandlerServiceClient` is wired in, this will
    /// attempt a real gRPC connection.
    pub async fn connect(&mut self) -> Result<()> {
        // Full implementation will instantiate HandlerServiceClient here:
        //   let client = HandlerServiceClient::connect(self.api_addr.clone()).await?;
        self.connected = true;
        tracing::info!("xray gRPC client connected to {}", self.api_addr);
        Ok(())
    }

    /// Check if the client is connected.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Get the API address.
    pub fn api_addr(&self) -> &str {
        &self.api_addr
    }

    /// Add a SOCKS5 inbound to xray-core via gRPC.
    ///
    /// The `inbound_json` is the xray inbound config as a JSON value.
    /// In the full implementation, this will be encoded as a protobuf
    /// `AddInboundRequest` and sent to the `HandlerService`.
    pub async fn add_inbound(&self, _inbound_json: &serde_json::Value) -> Result<()> {
        if !self.connected {
            anyhow::bail!("xray gRPC client not connected");
        }
        tracing::debug!("add_inbound called (stub)");
        // Full: encode TypedMessage, call HandlerServiceClient::add_inbound()
        Ok(())
    }

    /// Remove an inbound from xray-core via gRPC.
    pub async fn remove_inbound(&self, tag: &str) -> Result<()> {
        if !self.connected {
            anyhow::bail!("xray gRPC client not connected");
        }
        tracing::debug!("remove_inbound called for tag={tag} (stub)");
        // Full: send RemoveInboundRequest { tag }
        Ok(())
    }

    /// Add an outbound to xray-core via gRPC.
    ///
    /// The `outbound_json` is the xray outbound config as a JSON value.
    pub async fn add_outbound(&self, _outbound_json: &serde_json::Value) -> Result<()> {
        if !self.connected {
            anyhow::bail!("xray gRPC client not connected");
        }
        tracing::debug!("add_outbound called (stub)");
        // Full: encode TypedMessage, call HandlerServiceClient::add_outbound()
        Ok(())
    }

    /// Remove an outbound from xray-core via gRPC.
    pub async fn remove_outbound(&self, tag: &str) -> Result<()> {
        if !self.connected {
            anyhow::bail!("xray gRPC client not connected");
        }
        tracing::debug!("remove_outbound called for tag={tag} (stub)");
        // Full: send RemoveOutboundRequest { tag }
        Ok(())
    }
}
