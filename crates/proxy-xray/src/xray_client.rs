//! gRPC client for xray-core's HandlerService.
//!
//! Uses a hybrid approach:
//! - **Add operations**: Calls `xray api adi/ado` CLI commands (which handle
//!   JSON to protobuf TypedMessage conversion internally)
//! - **Remove operations**: Uses direct gRPC calls (only needs a tag string)
//!
//! Also provides automatic reconnection with exponential backoff and broadcasts
//! connection state via a `watch` channel.

use crate::proto::xray::app::proxyman::command::handler_service_client::HandlerServiceClient;
use crate::proto::xray::app::proxyman::command::{RemoveInboundRequest, RemoveOutboundRequest};
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::{RwLock, watch};

/// Client for communicating with xray-core's gRPC HandlerService.
pub struct XrayClient {
    api_addr: String,
    api_port: u16,
    binary_path: String,
    grpc_client: Option<HandlerServiceClient<tonic::transport::Channel>>,
    /// Sends `true` when the gRPC connection is established, `false` when lost.
    connected_tx: watch::Sender<bool>,
    /// Receiver for the connection state — can be cloned and passed to consumers.
    connected_rx: watch::Receiver<bool>,
}

impl XrayClient {
    /// Create a new `XrayClient` targeting the given gRPC API port.
    pub fn new(api_port: u16, binary_path: &str) -> Self {
        let (connected_tx, connected_rx) = watch::channel(false);
        Self {
            api_addr: format!("http://127.0.0.1:{api_port}"),
            api_port,
            binary_path: binary_path.to_string(),
            grpc_client: None,
            connected_tx,
            connected_rx,
        }
    }

    /// Try to connect to the gRPC API.
    ///
    /// On success, broadcasts the connected state via the watch channel.
    /// On failure, the state remains disconnected.
    pub async fn connect(&mut self) -> Result<()> {
        match HandlerServiceClient::connect(self.api_addr.clone()).await {
            Ok(client) => {
                self.grpc_client = Some(client);
                self.connected_tx.send(true).ok();
                tracing::info!("xray gRPC client connected to {}", self.api_addr);
                Ok(())
            }
            Err(e) => {
                tracing::warn!("xray gRPC connect failed: {e}, will retry on next cycle");
                self.connected_tx.send(false).ok();
                Err(anyhow::anyhow!("gRPC connect failed: {e}"))
            }
        }
    }

    /// Check if the client has an active gRPC connection.
    pub fn is_connected(&self) -> bool {
        self.grpc_client.is_some()
    }

    /// Return the current connection state from the watch channel.
    pub fn is_connected_watch(&self) -> bool {
        *self.connected_rx.borrow()
    }

    /// Return a clone of the connection state receiver.
    ///
    /// Consumers can use this to react to connection state changes
    /// (e.g., pause sync when disconnected, resume on reconnect).
    pub fn connected_rx(&self) -> watch::Receiver<bool> {
        self.connected_rx.clone()
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

    /// Handle a gRPC error from a remove operation.
    ///
    /// If the error is `Unavailable`, marks the connection as disconnected
    /// and clears the gRPC client. Returns an `anyhow::Error` in all cases.
    fn handle_grpc_error(&mut self, op: &str, tag: &str, status: tonic::Status) -> anyhow::Error {
        if status.code() == tonic::Code::Unavailable {
            self.connected_tx.send(false).ok();
            self.grpc_client = None;
            tracing::warn!("xray gRPC connection lost ({op})");
        }
        anyhow::anyhow!("gRPC error {op} {tag}: {status}")
    }

    /// Remove an inbound from xray-core via gRPC.
    ///
    /// If the gRPC call fails with a transport error (`Unavailable`), the
    /// connection is marked as disconnected and the gRPC client is cleared.
    pub async fn remove_inbound(&mut self, tag: &str) -> Result<()> {
        if let Some(ref mut client) = self.grpc_client {
            let req = RemoveInboundRequest {
                tag: tag.to_string(),
            };
            match client.remove_inbound(req).await {
                Ok(_) => {
                    tracing::debug!("removed inbound: {tag}");
                    Ok(())
                }
                Err(status) => Err(self.handle_grpc_error("removing inbound", tag, status)),
            }
        } else {
            anyhow::bail!("xray gRPC client not connected");
        }
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
    ///
    /// If the gRPC call fails with a transport error (`Unavailable`), the
    /// connection is marked as disconnected and the gRPC client is cleared.
    pub async fn remove_outbound(&mut self, tag: &str) -> Result<()> {
        if let Some(ref mut client) = self.grpc_client {
            let req = RemoveOutboundRequest {
                tag: tag.to_string(),
            };
            match client.remove_outbound(req).await {
                Ok(_) => {
                    tracing::debug!("removed outbound: {tag}");
                    Ok(())
                }
                Err(status) => Err(self.handle_grpc_error("removing outbound", tag, status)),
            }
        } else {
            anyhow::bail!("xray gRPC client not connected");
        }
    }

    /// Check if the gRPC connection is healthy by performing a lightweight call.
    ///
    /// Uses `ListInbounds` (or falls back to `remove_inbound` with a sentinel
    /// tag) to verify the connection is alive. Returns `true` if the call
    /// succeeds or returns a non-transport error (e.g., not found), and
    /// `false` if the transport layer reports the connection as unavailable.
    async fn health_check(&mut self) -> bool {
        match &mut self.grpc_client {
            Some(client) => {
                let req = RemoveInboundRequest {
                    tag: "__health_check__".to_string(),
                };
                match client.remove_inbound(req).await {
                    Ok(_) => true,
                    Err(status) => {
                        // NotFound is expected (sentinel tag doesn't exist) —
                        // it means the connection is alive.
                        // Unavailable means the connection is dead.
                        status.code() != tonic::Code::Unavailable
                    }
                }
            }
            None => false,
        }
    }

    /// Run the gRPC reconnection loop with exponential backoff.
    ///
    /// * If the client reports as connected: sleeps for 5 seconds, then runs
    ///   a health check. If the health check fails, the client is marked as
    ///   disconnected and a reconnect is attempted in the next iteration.
    /// * If the client reports as disconnected: attempts to reconnect with
    ///   exponential backoff (1s → 2s → 4s → ... → 30s max).
    ///
    /// Runs until the `shutdown` watch channel receives a shutdown signal or
    /// all senders are dropped.
    pub async fn reconnect_loop(client: Arc<RwLock<Self>>, mut shutdown: watch::Receiver<bool>) {
        let mut backoff = Duration::from_secs(1);
        let max_backoff = Duration::from_secs(30);

        tracing::info!("xray gRPC reconnect loop started");

        loop {
            // Decide sleep duration based on connection state.
            // We use a read lock here so other consumers are not blocked.
            let sleep_dur = {
                let guard = client.read().await;
                if *guard.connected_rx.borrow() {
                    Duration::from_secs(5)
                } else {
                    backoff
                }
            };

            tokio::select! {
                _ = shutdown.changed() => {
                    tracing::info!("xray gRPC reconnect loop shutting down");
                    break;
                }
                _ = tokio::time::sleep(sleep_dur) => {
                    // Woken up — check or restore connection.
                    let is_connected = {
                        let guard = client.read().await;
                        *guard.connected_rx.borrow()
                    };

                    if is_connected {
                        // Health check: briefly acquire write lock.
                        let mut guard = client.write().await;
                        if !guard.health_check().await {
                            tracing::warn!("xray gRPC health check failed, marking disconnected");
                            guard.connected_tx.send(false).ok();
                            guard.grpc_client = None;
                        }
                        // On success, reset backoff (next iteration uses 5s sleep).
                        backoff = Duration::from_secs(1);
                    } else {
                        // Reconnect attempt: acquire write lock.
                        let result = {
                            let mut guard = client.write().await;
                            guard.connect().await
                        };
                        match result {
                            Ok(()) => {
                                tracing::info!("xray gRPC reconnected");
                                backoff = Duration::from_secs(1);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "xray gRPC reconnect failed: {e}, retrying in {}s",
                                    backoff.as_secs()
                                );
                                // Sleep is handled by the select arm, so we just
                                // update backoff here.
                                backoff = (backoff * 2).min(max_backoff);
                            }
                        }
                    }
                }
            }
        }
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
