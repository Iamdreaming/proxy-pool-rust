//! gRPC client for xray-core's HandlerService.
//!
//! Uses a hybrid approach:
//! - **Add operations**: Calls `xray api adi/ado` CLI commands (which handle
//!   JSON to protobuf TypedMessage conversion internally)
//! - **Remove operations**: Uses direct gRPC calls (only needs a tag string)

use crate::proto::xray::app::proxyman::command::handler_service_client::HandlerServiceClient;
use crate::proto::xray::app::proxyman::command::{RemoveInboundRequest, RemoveOutboundRequest};
use anyhow::Result;
use tokio::process::Command;

/// Client for communicating with xray-core's gRPC HandlerService.
pub struct XrayClient {
    api_addr: String,
    api_port: u16,
    binary_path: String,
    grpc_client: Option<HandlerServiceClient<tonic::transport::Channel>>,
}

impl XrayClient {
    /// Create a new `XrayClient` targeting the given gRPC API port.
    pub fn new(api_port: u16, binary_path: &str) -> Self {
        Self {
            api_addr: format!("http://127.0.0.1:{api_port}"),
            api_port,
            binary_path: binary_path.to_string(),
            grpc_client: None,
        }
    }

    /// Try to connect to the gRPC API.
    pub async fn connect(&mut self) -> Result<()> {
        match HandlerServiceClient::connect(self.api_addr.clone()).await {
            Ok(client) => {
                self.grpc_client = Some(client);
                tracing::info!("xray gRPC client connected to {}", self.api_addr);
                Ok(())
            }
            Err(e) => {
                tracing::warn!("xray gRPC connect failed: {e}, will retry on next cycle");
                Err(anyhow::anyhow!("gRPC connect failed: {e}"))
            }
        }
    }

    /// Check if the client has an active gRPC connection.
    pub fn is_connected(&self) -> bool {
        self.grpc_client.is_some()
    }

    /// Get the API address.
    pub fn api_addr(&self) -> &str {
        &self.api_addr
    }

    /// Add a SOCKS5 inbound to xray-core via `xray api adi` CLI command.
    ///
    /// The `inbound_json` is a single inbound config object. It will be
    /// wrapped in `{"inbounds": [...]}` format before being written to a
    /// temp file and passed to the CLI.
    pub async fn add_inbound(&self, inbound_json: &serde_json::Value) -> Result<()> {
        let wrapper = serde_json::json!({
            "inbounds": [inbound_json]
        });
        self.execute_cli_api("adi", &wrapper).await
    }

    /// Remove an inbound from xray-core via gRPC.
    pub async fn remove_inbound(&mut self, tag: &str) -> Result<()> {
        if let Some(ref mut client) = self.grpc_client {
            let req = RemoveInboundRequest {
                tag: tag.to_string(),
            };
            client.remove_inbound(req).await?;
            tracing::debug!("removed inbound: {tag}");
        } else {
            anyhow::bail!("xray gRPC client not connected");
        }
        Ok(())
    }

    /// Add an outbound to xray-core via `xray api ado` CLI command.
    ///
    /// The `outbound_json` is a single outbound config object. It will be
    /// wrapped in `{"outbounds": [...]}` format.
    pub async fn add_outbound(&self, outbound_json: &serde_json::Value) -> Result<()> {
        let wrapper = serde_json::json!({
            "outbounds": [outbound_json]
        });
        self.execute_cli_api("ado", &wrapper).await
    }

    /// Remove an outbound from xray-core via gRPC.
    pub async fn remove_outbound(&mut self, tag: &str) -> Result<()> {
        if let Some(ref mut client) = self.grpc_client {
            let req = RemoveOutboundRequest {
                tag: tag.to_string(),
            };
            client.remove_outbound(req).await?;
            tracing::debug!("removed outbound: {tag}");
        } else {
            anyhow::bail!("xray gRPC client not connected");
        }
        Ok(())
    }

    /// Execute an `xray api {subcommand}` CLI command with JSON config.
    ///
    /// Writes the JSON to a temp file, runs the CLI command, and checks
    /// the exit code.
    async fn execute_cli_api(&self, subcommand: &str, json: &serde_json::Value) -> Result<()> {
        // Write JSON to temp file
        let temp_dir = std::env::temp_dir().join("proxy-pool-xray-api");
        std::fs::create_dir_all(&temp_dir)?;

        let file_name = format!("xray-api-{}-{}.json", subcommand, std::process::id());
        let temp_path = temp_dir.join(file_name);
        let json_str = serde_json::to_string_pretty(json)?;
        std::fs::write(&temp_path, &json_str)?;

        let server_addr = format!("127.0.0.1:{}", self.api_port);

        let output = Command::new(&self.binary_path)
            .arg("api")
            .arg(subcommand)
            .arg("--server")
            .arg(&server_addr)
            .arg(&temp_path)
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("failed to execute xray api {subcommand}: {e}"))?;

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "xray api {subcommand} failed with exit code {}: {stderr}",
                output.status.code().unwrap_or(-1)
            );
        }

        tracing::debug!("xray api {subcommand} succeeded");
        Ok(())
    }
}
