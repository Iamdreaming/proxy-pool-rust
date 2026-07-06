//! Scheduler: runs the fetch → dedup → validate → store pipeline periodically.

use crate::circuit;
use crate::config::PoolSettings;
use crate::dedup;
use crate::fetcher::Fetcher;
use crate::fetcher::base::{FetcherOutput, FetcherRunReport};
use crate::geoip::GeoIPLookup;
use crate::models::{Protocol, Proxy};
use crate::store::ProxyStore;
use crate::validator::Validator;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};

/// Result of a scheduler refresh cycle.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SchedulerResult {
    pub fetched: usize,
    pub validated: usize,
    pub stored: usize,
    pub errors: usize,
    pub fetchers: Vec<FetcherRunReport>,
}

/// Commands that can be sent to the scheduler from other tasks.
pub enum SchedulerCommand {
    /// Trigger a single fetch+validate+store cycle and report the result.
    Refresh {
        reply: oneshot::Sender<SchedulerResult>,
    },
    /// Trigger one fetcher by id or unique name and report the result.
    RefreshFetcher {
        fetcher_id: String,
        reply: oneshot::Sender<anyhow::Result<SchedulerResult>>,
    },
}

/// Handle for sending commands to the scheduler from other tasks.
#[derive(Clone)]
pub struct SchedulerHandle {
    cmd_tx: mpsc::Sender<SchedulerCommand>,
    fetcher_statuses: Arc<RwLock<Vec<FetcherRunReport>>>,
}

impl SchedulerHandle {
    /// Create a new handle from a channel sender.
    pub fn new(cmd_tx: mpsc::Sender<SchedulerCommand>) -> Self {
        Self {
            cmd_tx,
            fetcher_statuses: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a handle wired to the scheduler's fetcher status snapshot.
    pub fn with_fetcher_statuses(
        cmd_tx: mpsc::Sender<SchedulerCommand>,
        fetcher_statuses: Arc<RwLock<Vec<FetcherRunReport>>>,
    ) -> Self {
        Self {
            cmd_tx,
            fetcher_statuses,
        }
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

    /// Trigger a refresh cycle for one configured fetcher id.
    pub async fn refresh_fetcher(
        &self,
        fetcher_id: impl Into<String>,
    ) -> anyhow::Result<SchedulerResult> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SchedulerCommand::RefreshFetcher {
                fetcher_id: fetcher_id.into(),
                reply: tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("scheduler channel closed"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("scheduler result dropped"))?
    }

    /// Return the latest run report for each configured fetcher.
    pub async fn fetcher_statuses(&self) -> Vec<FetcherRunReport> {
        let now = chrono::Utc::now();
        self.fetcher_statuses
            .read()
            .await
            .clone()
            .into_iter()
            .map(|report| report.with_effective_circuit_state(now))
            .collect()
    }
}

/// Runs the fetch → dedup → validate → store pipeline on a schedule.
pub struct Scheduler {
    fetchers: Vec<Arc<dyn Fetcher>>,
    validator: Arc<Validator>,
    store: Arc<ProxyStore>,
    settings: PoolSettings,
    geoip: Option<Arc<Mutex<GeoIPLookup>>>,
    fetcher_statuses: Arc<RwLock<Vec<FetcherRunReport>>>,
}

impl Scheduler {
    pub fn new(
        fetchers: Vec<Arc<dyn Fetcher>>,
        validator: Validator,
        store: Arc<ProxyStore>,
        settings: PoolSettings,
        geoip: Option<Arc<Mutex<GeoIPLookup>>>,
    ) -> Self {
        let fetcher_statuses = Arc::new(RwLock::new(
            fetchers
                .iter()
                .map(|f| FetcherRunReport::never_run(f.as_ref()))
                .collect(),
        ));

        Self {
            fetchers,
            validator: Arc::new(validator),
            store,
            settings,
            geoip,
            fetcher_statuses,
        }
    }

    /// Shared latest fetcher status snapshot for API/MCP handles.
    pub fn fetcher_statuses(&self) -> Arc<RwLock<Vec<FetcherRunReport>>> {
        self.fetcher_statuses.clone()
    }

    /// One full pipeline pass: fetch all → dedup → filter circuit-broken → validate → store.
    ///
    /// Returns a `SchedulerResult` with counts of fetched, validated, stored proxies and errors.
    pub async fn run_once(&self) -> SchedulerResult {
        self.run_selected(None).await.unwrap_or_else(|e| {
            tracing::warn!("fetch cycle selection failed: {e}");
            SchedulerResult {
                errors: 1,
                ..SchedulerResult::default()
            }
        })
    }

    /// One full pipeline pass for one configured fetcher.
    pub async fn run_one_fetcher(&self, fetcher_id: &str) -> anyhow::Result<SchedulerResult> {
        self.run_selected(Some(fetcher_id)).await
    }

    async fn run_selected(&self, fetcher_id: Option<&str>) -> anyhow::Result<SchedulerResult> {
        let mut result = SchedulerResult::default();

        // 1. Fetch from all sources concurrently
        let selected: Vec<_> = self
            .fetchers
            .iter()
            .filter(|fetcher| match fetcher_id {
                Some(id) => fetcher_matches(fetcher.as_ref(), id),
                None => true,
            })
            .collect();

        if selected.is_empty() {
            let id = fetcher_id.unwrap_or_default();
            return Err(anyhow::anyhow!("fetcher not found: {id}"));
        }

        let manual_refresh = fetcher_id.is_some();
        let now = chrono::Utc::now();
        let previous_by_id: HashMap<String, FetcherRunReport> = self
            .fetcher_statuses
            .read()
            .await
            .iter()
            .cloned()
            .map(|report| (report.id.clone(), report))
            .collect();
        let mut runnable = Vec::new();

        for fetcher in selected {
            let id = fetcher.id();
            if should_skip_fetcher_for_run(manual_refresh, previous_by_id.get(&id), now) {
                let previous = previous_by_id
                    .get(&id)
                    .expect("skip decision requires previous report");
                result.fetchers.push(FetcherRunReport::skipped_open(
                    fetcher.as_ref(),
                    previous,
                    now,
                ));
                continue;
            }
            runnable.push(fetcher);
        }

        let outputs: Vec<FetcherOutput> =
            futures::future::join_all(runnable.iter().map(|f| f.fetch_with_report())).await;

        let mut all_proxies = Vec::new();
        for output in outputs {
            let previous = previous_by_id.get(&output.report.id);
            let report = output.report.apply_circuit_transition(
                previous,
                manual_refresh,
                chrono::Utc::now(),
            );
            if report.status == crate::fetcher::base::FetcherRunStatus::Error {
                result.errors += 1;
            }
            result.fetchers.push(report);
            all_proxies.extend(output.proxies);
        }
        self.update_fetcher_statuses(&result.fetchers).await;
        result.fetched = all_proxies.len();

        // 2. Dedup within this batch
        let unique = dedup::dedup(all_proxies);
        tracing::info!("fetched proxies ({} unique after dedup)", unique.len());
        if unique.is_empty() {
            return Ok(result);
        }

        // 3. Filter out proxies whose circuit breaker is still open in the store
        let candidates = self.filter_circuit_broken(unique).await;
        let skipped = result.fetched - candidates.len();
        if skipped > 0 {
            tracing::info!("skipped {} circuit-broken proxies", skipped);
        }

        // 4. Validate concurrently
        let mut working = self
            .validator
            .validate_many(&candidates, self.settings.validate_concurrency)
            .await;
        result.validated = working.len();
        tracing::info!("validated {} working proxies", working.len());

        // 4b. Enrich working proxies with GeoIP data
        if let Some(ref geoip_mutex) = self.geoip {
            let mut geoip = geoip_mutex.lock().await;
            for p in &mut working {
                let info = geoip.lookup(&p.host).await;
                p.is_overseas = geoip.is_overseas(&info.country);
                p.country = Some(info.country);
                p.country_name = Some(info.country_name);
            }
            tracing::info!("enriched {} proxies with geoip data", working.len());
        }

        // 5. Store
        for p in &working {
            if let Err(e) = self.store.add(p).await {
                tracing::warn!("failed to store proxy {}: {e}", p.key());
                result.errors += 1;
            }
        }
        result.stored = working.len() - result.errors;

        Ok(result)
    }

    async fn update_fetcher_statuses(&self, reports: &[FetcherRunReport]) {
        let mut statuses = self.fetcher_statuses.write().await;
        for report in reports {
            if let Some(existing) = statuses.iter_mut().find(|s| s.id == report.id) {
                *existing = report.clone();
            } else {
                statuses.push(report.clone());
            }
        }
    }

    /// Remove newly-fetched proxies whose circuit breaker is still open in the store.
    ///
    /// Queries each protocol's ZSET for matching entries and checks their circuit state.
    /// Proxies whose circuit has transitioned to half-open (recovery window) are kept
    /// so they get a chance to prove themselves during validation.
    async fn filter_circuit_broken(&self, proxies: Vec<Proxy>) -> Vec<Proxy> {
        // Collect protocols present in this batch
        let protocols: std::collections::HashSet<Protocol> =
            proxies.iter().map(|p| p.protocol).collect();

        // Build a set of dedup_keys for proxies that are circuit-open in the store
        let mut blocked = std::collections::HashSet::new();
        for protocol in protocols {
            match self.store.all(protocol).await {
                Ok(existing) => {
                    for stored in existing {
                        if circuit::is_circuit_open(&stored) {
                            blocked.insert(stored.dedup_key());
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "failed to query store for {protocol} during circuit filter: {e}"
                    );
                }
            }
        }

        proxies
            .into_iter()
            .filter(|p| !blocked.contains(&p.dedup_key()))
            .collect()
    }

    /// Re-validate proxies already in the store; drop dead ones.
    pub async fn revalidate_existing(&self) -> anyhow::Result<()> {
        for protocol in [Protocol::Http, Protocol::Https, Protocol::Socks5] {
            let existing = self.store.all(protocol).await?;
            if existing.is_empty() {
                continue;
            }

            let working = self
                .validator
                .validate_many(&existing, self.settings.validate_concurrency)
                .await;

            let working_keys: std::collections::HashSet<String> =
                working.iter().map(|p| p.key()).collect();

            for p in &working {
                if let Err(e) = self.store.add(p).await {
                    tracing::warn!(
                        "failed to update successful validation for {}: {e}",
                        p.key()
                    );
                }
            }

            for p in &existing {
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
                        SchedulerCommand::RefreshFetcher { fetcher_id, reply } => {
                            let result = this.run_one_fetcher(&fetcher_id).await;
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

fn fetcher_matches(fetcher: &dyn Fetcher, requested: &str) -> bool {
    fetcher.id().eq_ignore_ascii_case(requested) || fetcher.name().eq_ignore_ascii_case(requested)
}

fn should_skip_fetcher_for_run(
    manual_refresh: bool,
    previous: Option<&FetcherRunReport>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    !manual_refresh && previous.is_some_and(|report| report.should_skip_automatic_at(now))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetcher::base::FetcherCircuitState;
    use chrono::{Duration, Utc};
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
            fetchers: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"fetched\":10"));
        assert!(json.contains("\"validated\":5"));
        assert!(json.contains("\"fetchers\""));
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
                        fetchers: vec![],
                    })
                    .unwrap();
            }
            SchedulerCommand::RefreshFetcher { .. } => panic!("expected refresh command"),
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

    #[test]
    fn automatic_refresh_skips_open_fetcher_but_manual_refresh_probes() {
        let now = Utc::now();
        let report = FetcherRunReport {
            id: "source".into(),
            name: "Source".into(),
            status: crate::fetcher::base::FetcherRunStatus::Error,
            fetched: 0,
            parsed: 0,
            error: Some("timeout".into()),
            circuit_state: FetcherCircuitState::Open,
            consecutive_failures: 3,
            last_error: Some("timeout".into()),
            last_attempt_at: None,
            last_success_at: None,
            opened_at: Some(now),
            next_probe_at: Some(now + Duration::seconds(60)),
            action: None,
            started_at: None,
            finished_at: None,
            duration_ms: None,
        };

        assert!(should_skip_fetcher_for_run(false, Some(&report), now));
        assert!(!should_skip_fetcher_for_run(true, Some(&report), now));
    }
}
