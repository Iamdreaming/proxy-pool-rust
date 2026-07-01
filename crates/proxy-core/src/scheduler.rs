//! Scheduler: runs the fetch → dedup → validate → store pipeline periodically.

use crate::config::PoolSettings;
use crate::dedup;
use crate::fetcher::Fetcher;
use crate::models::Protocol;
use crate::store::ProxyStore;
use crate::validator::Validator;
use std::sync::Arc;

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
    pub async fn run_once(&self) -> anyhow::Result<()> {
        // 1. Fetch from all sources concurrently
        let mut all_proxies = Vec::new();
        let results = futures::future::join_all(self.fetchers.iter().map(|f| f.fetch())).await;

        for proxies in results {
            all_proxies.extend(proxies);
        }

        // 2. Dedup
        let unique = dedup::dedup(all_proxies);
        tracing::info!("fetched proxies ({} unique after dedup)", unique.len());
        if unique.is_empty() {
            return Ok(());
        }

        // 3. Validate concurrently
        let working = self
            .validator
            .validate_many(&unique, self.settings.validate_concurrency)
            .await;
        tracing::info!("validated {} working proxies", working.len());

        // 4. Store
        for p in &working {
            if let Err(e) = self.store.add(p).await {
                tracing::warn!("failed to store proxy {}: {e}", p.key());
            }
        }

        Ok(())
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
                    && let Err(e) = self.store.mark_failed(p).await
                {
                    tracing::warn!("failed to mark {} as failed: {e}", p.key());
                }
            }
        }
        Ok(())
    }

    /// Run the scheduler loops (fetch + validate) until cancelled.
    pub async fn run(self: Arc<Self>) {
        let fetch_interval = std::time::Duration::from_secs(self.settings.fetch_interval_sec);
        let validate_interval = std::time::Duration::from_secs(self.settings.validate_interval_sec);

        let this_fetch = self.clone();
        let fetch_loop = tokio::spawn(async move {
            loop {
                if let Err(e) = this_fetch.run_once().await {
                    tracing::error!("fetch loop error: {e}");
                }
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

        let _ = tokio::join!(fetch_loop, validate_loop);
    }
}
