//! Scheduler: runs the fetch → dedup → validate → store pipeline periodically.

use crate::config::PoolSettings;
use crate::dedup;
use crate::fetcher::Fetcher;
use crate::models::Protocol;
use crate::store::ProxyStore;
use crate::validator::Validator;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// Result of a scheduler refresh cycle.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SchedulerResult {
    pub fetched: usize,
    pub validated: usize,
    pub stored: usize,
    pub errors: usize,
}

/// Commands that can be sent to the scheduler from other tasks.
pub enum SchedulerCommand {
    /// Trigger a single fetch+validate+store cycle and report the result.
    Refresh {
        reply: oneshot::Sender<SchedulerResult>,
    },
}

/// Handle for sending commands to the scheduler from other tasks.
#[derive(Clone)]
pub struct SchedulerHandle {
    cmd_tx: mpsc::Sender<SchedulerCommand>,
}

impl SchedulerHandle {
    /// Create a new handle from a channel sender.
    pub fn new(cmd_tx: mpsc::Sender<SchedulerCommand>) -> Self {
        Self { cmd_tx }
    }

    /// Trigger a refresh cycle and wait for the result.
    pub async fn refresh(&self) -> anyhow::Result<SchedulerResult> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SchedulerCommand::Refresh { reply: tx })
            .await
            .map_err(|_| anyhow::anyhow!("scheduler channel closed"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("scheduler result dropped"))
    }
}

/// Runs the fetch → dedup → validate → store pipeline on a schedule.
pub struct Scheduler {
    fetchers: Vec<Arc<dyn Fetcher>>,
    validator: Arc<Validator>,
    store: Arc<ProxyStore>,
    settings: PoolSettings,
}

impl Scheduler {
    pub fn new(
        fetchers: Vec<Arc<dyn Fetcher>>,
        validator: Validator,
        store: Arc<ProxyStore>,
        settings: PoolSettings,
    ) -> Self {
        Self {
            fetchers,
            validator: Arc::new(validator),
            store,
            settings,
        }
    }

    /// One full pipeline pass: fetch all → dedup → validate → store.
    ///
    /// Returns a `SchedulerResult` with counts of fetched, validated, stored proxies and errors.
    pub async fn run_once(&self) -> SchedulerResult {
        let mut result = SchedulerResult::default();

        // 1. Fetch from all sources concurrently
        let mut all_proxies = Vec::new();
        let results = futures::future::join_all(self.fetchers.iter().map(|f| f.fetch())).await;

        for proxies in results {
            all_proxies.extend(proxies);
        }
        result.fetched = all_proxies.len();

        // 2. Dedup
        let unique = dedup::dedup(all_proxies);
        tracing::info!("fetched proxies ({} unique after dedup)", unique.len());
        if unique.is_empty() {
            return result;
        }

        // 3. Validate concurrently
        let working = self
            .validator
            .validate_many(&unique, self.settings.validate_concurrency)
            .await;
        result.validated = working.len();
        tracing::info!("validated {} working proxies", working.len());

        // 4. Store
        for p in &working {
            if let Err(e) = self.store.add(p).await {
                tracing::warn!("failed to store proxy {}: {e}", p.key());
                result.errors += 1;
            }
        }
        result.stored = working.len() - result.errors;

        result
    }

    /// Re-validate proxies already in the store; drop dead ones.
    pub async fn revalidate_existing(&self) -> anyhow::Result<()> {
        for protocol in [Protocol::Http, Protocol::Https, Protocol::Socks5] {
            let existing = self.store.all(protocol).await?;
            if existing.is_empty() {
                continue;
            }

            // Reset counts before re-validation
            let reset: Vec<_> = existing
                .into_iter()
                .map(|mut p| {
                    p.success_count = 0;
                    p.fail_count = 0;
                    p
                })
                .collect();

            let working = self
                .validator
                .validate_many(&reset, self.settings.validate_concurrency)
                .await;

            let working_keys: std::collections::HashSet<String> =
                working.iter().map(|p| p.key()).collect();

            for p in &reset {
                if !working_keys.contains(&p.key())
                    && let Err(e) = self.store.mark_failed_with_circuit(p).await
                {
                    tracing::warn!("failed to mark {} as failed: {e}", p.key());
                }
            }
        }
        Ok(())
    }

    /// Run the scheduler loops (fetch + validate) until cancelled.
    ///
    /// If `cmd_rx` is provided, also listens for external commands (e.g. refresh requests)
    /// on the channel and handles them concurrently with the periodic loops.
    pub async fn run(self: Arc<Self>, cmd_rx: Option<mpsc::Receiver<SchedulerCommand>>) {
        let fetch_interval = std::time::Duration::from_secs(self.settings.fetch_interval_sec);
        let validate_interval = std::time::Duration::from_secs(self.settings.validate_interval_sec);

        let this_fetch = self.clone();
        let fetch_loop = tokio::spawn(async move {
            loop {
                let result = this_fetch.run_once().await;
                tracing::info!(
                    "fetch cycle: fetched={}, validated={}, stored={}, errors={}",
                    result.fetched,
                    result.validated,
                    result.stored,
                    result.errors,
                );
                tokio::time::sleep(fetch_interval).await;
            }
        });

        let this_validate = self.clone();
        let validate_loop = tokio::spawn(async move {
            loop {
                tokio::time::sleep(validate_interval).await;
                if let Err(e) = this_validate.revalidate_existing().await {
                    tracing::error!("validate loop error: {e}");
                }
            }
        });

        // Command listener: handle external refresh requests
        let mut handles = vec![fetch_loop, validate_loop];
        if let Some(mut rx) = cmd_rx {
            let this = self.clone();
            handles.push(tokio::spawn(async move {
                while let Some(cmd) = rx.recv().await {
                    match cmd {
                        SchedulerCommand::Refresh { reply } => {
                            let result = this.run_once().await;
                            let _ = reply.send(result);
                        }
                    }
                }
            }));
        }
        for h in handles {
            let _ = h.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[test]
    fn test_scheduler_result_default() {
        let result = SchedulerResult::default();
        assert_eq!(result.fetched, 0);
        assert_eq!(result.validated, 0);
        assert_eq!(result.stored, 0);
        assert_eq!(result.errors, 0);
    }

    #[test]
    fn test_scheduler_result_serialize() {
        let result = SchedulerResult {
            fetched: 10,
            validated: 5,
            stored: 4,
            errors: 1,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"fetched\":10"));
        assert!(json.contains("\"validated\":5"));
        assert!(json.contains("\"stored\":4"));
        assert!(json.contains("\"errors\":1"));
    }

    #[tokio::test]
    async fn test_scheduler_handle_refresh() {
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<SchedulerCommand>(8);
        let handle = SchedulerHandle::new(cmd_tx);

        let handle_clone = handle.clone();
        let refresh_task = tokio::spawn(async move { handle_clone.refresh().await });

        let cmd = cmd_rx.recv().await.unwrap();
        match cmd {
            SchedulerCommand::Refresh { reply } => {
                reply
                    .send(SchedulerResult {
                        fetched: 100,
                        validated: 50,
                        stored: 45,
                        errors: 5,
                    })
                    .unwrap();
            }
        }

        let received = refresh_task.await.unwrap().unwrap();
        assert_eq!(received.fetched, 100);
        assert_eq!(received.validated, 50);
        assert_eq!(received.stored, 45);
        assert_eq!(received.errors, 5);
    }

    #[tokio::test]
    async fn test_scheduler_handle_closed_channel() {
        let (cmd_tx, cmd_rx) = mpsc::channel::<SchedulerCommand>(8);
        let handle = SchedulerHandle::new(cmd_tx);

        drop(cmd_rx);

        let result = handle.refresh().await;
        assert!(result.is_err());
    }
}
