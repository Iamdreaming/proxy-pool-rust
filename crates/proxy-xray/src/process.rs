//! Manages an xray-core subprocess with supervision and restart.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::process::{Child, Command};

/// Manages an xray-core subprocess.
///
/// Handles starting, monitoring, killing, and restarting the xray-core process.
/// On drop, the subprocess is killed automatically.
pub struct XrayProcess {
    child: Option<Child>,
    binary_path: String,
    config_path: PathBuf,
    api_port: u16,
    restart_count: Arc<AtomicU32>,
}

impl XrayProcess {
    /// Start xray-core with the given config file.
    ///
    /// The process is spawned with piped stdout/stderr for log capture.
    pub async fn start(
        binary_path: &str,
        config_path: &Path,
        api_port: u16,
    ) -> anyhow::Result<Self> {
        let mut child = Command::new(binary_path)
            .arg("run")
            .arg("-c")
            .arg(config_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn xray-core: {e}"))?;

        // Give the process a brief moment to start (or fail immediately).
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(anyhow::anyhow!(
                    "xray-core exited immediately with status: {status}"
                ));
            }
            Ok(None) => {
                // Still running — good.
            }
            Err(e) => {
                return Err(anyhow::anyhow!("failed to check xray-core status: {e}"));
            }
        }

        tracing::info!(
            "xray-core started: {} -c {} (api_port={api_port})",
            binary_path,
            config_path.display()
        );

        Ok(Self {
            child: Some(child),
            binary_path: binary_path.to_string(),
            config_path: config_path.to_path_buf(),
            api_port,
            restart_count: Arc::new(AtomicU32::new(0)),
        })
    }

    /// Check if the process is still running.
    pub fn is_running(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(_status)) => false, // exited
                Ok(None) => true,           // still running
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Try to restart the process.
    ///
    /// Kills the existing process first, then spawns a new one with the same
    /// config path and binary.
    pub async fn restart(&mut self) -> anyhow::Result<()> {
        self.kill().await;

        let mut child = Command::new(&self.binary_path)
            .arg("run")
            .arg("-c")
            .arg(&self.config_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to restart xray-core: {e}"))?;

        // Brief wait to detect immediate crash.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        if let Ok(Some(status)) = child.try_wait() {
            return Err(anyhow::anyhow!(
                "xray-core exited immediately after restart: {status}"
            ));
        }

        let count = self.restart_count.fetch_add(1, Ordering::Relaxed) + 1;
        tracing::info!("xray-core restarted (restart #{count})");
        self.child = Some(child);
        Ok(())
    }

    /// Kill the process and wait for it to exit (up to 5 seconds).
    pub async fn kill(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
        }
        self.child = None;
    }

    /// Run the supervision loop: monitor the subprocess and restart on crash.
    ///
    /// Uses exponential backoff (1s -> 2s -> 4s -> ... -> 60s max) between
    /// restarts.  Returns when the shutdown signal is received (all senders
    /// dropped or `true` sent).
    pub async fn supervise(&mut self, mut shutdown_rx: tokio::sync::watch::Receiver<bool>) {
        let mut backoff_secs: f64 = 1.0;
        const MAX_BACKOFF: f64 = 60.0;
        const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

        tracing::info!("xray supervisor started");

        loop {
            tokio::select! {
                _ = tokio::time::sleep(CHECK_INTERVAL) => {
                    if !self.is_running() {
                        let delay = std::time::Duration::from_secs_f64(backoff_secs);
                        tracing::warn!(
                            "xray-core died, restarting in {:.1}s (restart #{})",
                            backoff_secs,
                            self.restart_count() + 1
                        );
                        tokio::time::sleep(delay).await;
                        match self.restart().await {
                            Ok(()) => {
                                backoff_secs = 1.0;
                                tracing::info!("xray-core restarted successfully");
                            }
                            Err(e) => {
                                backoff_secs = (backoff_secs * 2.0).min(MAX_BACKOFF);
                                tracing::error!("xray-core restart failed: {e}");
                            }
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    tracing::info!("xray supervisor shutting down");
                    self.kill().await;
                    return;
                }
            }
        }
    }

    /// Get the number of restarts performed.
    pub fn restart_count(&self) -> u32 {
        self.restart_count.load(Ordering::Relaxed)
    }

    /// Get the API port.
    pub fn api_port(&self) -> u16 {
        self.api_port
    }

    /// Get the binary path.
    pub fn binary_path(&self) -> &str {
        &self.binary_path
    }

    /// Get the config file path.
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }
}

impl Drop for XrayProcess {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.start_kill();
        }
    }
}
