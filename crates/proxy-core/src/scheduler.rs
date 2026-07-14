//! Scheduler: runs the fetch → dedup → validate → store pipeline periodically.

use crate::capability::{CapabilityStore, CapabilityTag, CapabilityTarget};
use crate::circuit;
use crate::config::{CapabilityConfig, PoolSettings};
use crate::dedup;
use crate::fetcher::Fetcher;
use crate::fetcher::base::{FetcherOutput, FetcherRunReport};
use crate::geoip::GeoIPLookup;
use crate::models::{Protocol, Proxy};
use crate::store::ProxyStore;
use crate::validator::{ValidationTarget, Validator};
use std::collections::HashMap;
use std::str::FromStr;
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
    capability: CapabilityConfig,
    geoip: Option<Arc<Mutex<GeoIPLookup>>>,
    fetcher_statuses: Arc<RwLock<Vec<FetcherRunReport>>>,
}

impl Scheduler {
    pub fn new(
        fetchers: Vec<Arc<dyn Fetcher>>,
        validator: Validator,
        store: Arc<ProxyStore>,
        settings: PoolSettings,
        capability: CapabilityConfig,
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
            capability,
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
            let FetcherOutput {
                mut proxies,
                report,
            } = output;
            let source_id = report.id.clone();
            let previous = previous_by_id.get(&source_id);
            let report =
                report.apply_circuit_transition(previous, manual_refresh, chrono::Utc::now());
            if report.status == crate::fetcher::base::FetcherRunStatus::Error {
                result.errors += 1;
            }
            for proxy in &mut proxies {
                proxy.source = Some(source_id.clone());
            }
            result.fetchers.push(report);
            all_proxies.extend(proxies);
        }
        result.fetched = all_proxies.len();
        self.update_fetcher_statuses(&result.fetchers).await;

        // 2. Dedup within this batch
        let unique = dedup::dedup(all_proxies);
        let unique_by_source = count_by_source(&unique);
        tracing::info!("fetched proxies ({} unique after dedup)", unique.len());
        if unique.is_empty() {
            apply_source_quality_counts(
                &mut result.fetchers,
                &unique_by_source,
                &HashMap::new(),
                &HashMap::new(),
            );
            self.update_fetcher_statuses(&result.fetchers).await;
            return Ok(result);
        }

        // 3. Filter out proxies whose circuit breaker is still open in the store
        let unique_count = unique.len();
        let candidates = self.filter_circuit_broken(unique).await;
        let skipped = unique_count.saturating_sub(candidates.len());
        if skipped > 0 {
            tracing::info!("skipped {} circuit-broken proxies", skipped);
        }

        // 4. Validate concurrently
        let mut working = self.validate_candidates(&candidates).await;
        result.validated = working.len();
        let validated_by_source = count_by_source(&working);
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
        let mut stored_by_source = HashMap::new();
        for p in &working {
            if let Err(e) = self.store.add(p).await {
                tracing::warn!("failed to store proxy {}: {e}", p.key());
                result.errors += 1;
            } else {
                increment_source_count(&mut stored_by_source, p);
                result.stored += 1;
            }
        }
        apply_source_quality_counts(
            &mut result.fetchers,
            &unique_by_source,
            &validated_by_source,
            &stored_by_source,
        );
        self.update_fetcher_statuses(&result.fetchers).await;

        Ok(result)
    }

    async fn validate_candidates(&self, candidates: &[Proxy]) -> Vec<Proxy> {
        let targets: Vec<ValidationTarget> = self
            .settings
            .effective_validate_targets()
            .into_iter()
            .map(ValidationTarget::from)
            .collect();
        self.validator
            .validate_many_against_targets(
                candidates,
                &targets,
                self.settings.validate_concurrency,
                self.settings.target_admission.into(),
            )
            .await
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

            let targets: Vec<ValidationTarget> = self
                .settings
                .effective_validate_targets()
                .into_iter()
                .map(ValidationTarget::from)
                .collect();
            let outcomes = self
                .validator
                .validate_many_with_results(
                    &existing,
                    &targets,
                    self.settings.validate_concurrency,
                    self.settings.target_admission.into(),
                )
                .await;

            for outcome in &outcomes {
                if let Some(ref proxy) = outcome.alive_proxy
                    && let Err(e) = self.store.add(proxy).await
                {
                    tracing::warn!(
                        "failed to update successful validation for {}: {e}",
                        proxy.key()
                    );
                }
            }

            let alive_keys: std::collections::HashSet<String> = outcomes
                .iter()
                .filter_map(|o| o.alive_proxy.as_ref().map(|p| p.key()))
                .collect();

            for p in &existing {
                if !alive_keys.contains(&p.key()) {
                    let reason = outcomes
                        .iter()
                        .find(|o| o.proxy_key == p.key())
                        .and_then(|o| o.error_type.map(error_type_to_reason))
                        .unwrap_or("validation_failed");
                    if let Err(e) = self.store.mark_failed_with_circuit(p, reason).await {
                        tracing::warn!("failed to mark {} as failed: {e}", p.key());
                    }
                }
            }
        }

        if self.capability.enabled
            && self.capability.test_on_revalidate
            && let Err(e) = self.run_capability_tests().await
        {
            tracing::warn!("capability revalidation failed: {e}");
        }

        Ok(())
    }

    /// Probe top-K proxies against the configured capability targets and tag
    /// proxies that satisfy each target.
    ///
    /// Probes run bounded by a semaphore so a large candidate set cannot
    /// overwhelm the network. A failed probe never removes an existing tag;
    /// tags are only assigned on a successful probe.
    async fn run_capability_tests(&self) -> anyhow::Result<()> {
        let top_k = self.capability.top_k.max(1);
        let mut seen = std::collections::HashSet::new();
        let mut candidates: Vec<Proxy> = Vec::new();
        for protocol in [Protocol::Http, Protocol::Https, Protocol::Socks5] {
            match self.store.get_top_candidates(protocol, top_k, top_k).await {
                Ok(mut tops) => {
                    tops.retain(|p| seen.insert(p.key()));
                    candidates.extend(tops);
                }
                Err(e) => tracing::warn!("capability: failed to list {protocol:?} candidates: {e}"),
            }
        }
        if candidates.is_empty() {
            return Ok(());
        }

        let targets: Vec<CapabilityTarget> = self
            .capability
            .targets
            .iter()
            .filter_map(|t| {
                CapabilityTag::from_str(&t.tag)
                    .ok()
                    .map(|tag| CapabilityTarget {
                        name: t.name.clone(),
                        url: t.url.clone(),
                        expected_status: t.expected_status,
                        tag,
                    })
            })
            .collect();
        if targets.is_empty() {
            return Ok(());
        }

        let cap_store = CapabilityStore::new(self.store.raw_conn());
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
        let mut handles = Vec::new();
        for proxy in candidates {
            for target in targets.clone() {
                let sem = sem.clone();
                let cap = cap_store.clone();
                let proxy_key = proxy.key();
                let proxy = proxy.clone();
                handles.push(tokio::spawn(async move {
                    let _permit = sem.acquire().await.ok();
                    match cap.test_capability(&proxy, &target).await {
                        Ok(true) => {
                            if let Err(e) = cap.assign(&proxy_key, &target.tag).await {
                                tracing::warn!(
                                    proxy = %proxy_key,
                                    tag = %target.tag,
                                    "capability: assign failed: {e}"
                                );
                            } else {
                                tracing::info!(
                                    proxy = %proxy_key,
                                    tag = %target.tag,
                                    "capability: tagged proxy"
                                );
                            }
                        }
                        Ok(false) => {
                            tracing::debug!(
                                proxy = %proxy_key,
                                target = %target.name,
                                "capability: probe negative"
                            );
                        }
                        Err(e) => {
                            tracing::debug!(
                                proxy = %proxy_key,
                                target = %target.name,
                                "capability: probe error: {e}"
                            );
                        }
                    }
                }));
            }
        }
        for h in handles {
            let _ = h.await;
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
    if manual_refresh {
        return false;
    }
    let Some(report) = previous else {
        return false;
    };
    // Skip if the fetcher circuit is open (too many fetch errors)
    if report.should_skip_automatic_at(now) {
        return true;
    }
    // Skip if the last run had very low validation survival rate (< 10%)
    // This gates sources that successfully fetch proxies but those proxies
    // almost never pass validation, reducing wasted validation capacity.
    if let Some(survival_rate) = report.validation_survival_rate
        && survival_rate < 0.10
        && report.unique > 0
    {
        return true;
    }
    false
}

fn error_type_to_reason(error_type: crate::validator::ProxyCheckErrorType) -> &'static str {
    match error_type {
        crate::validator::ProxyCheckErrorType::Timeout => "timeout",
        crate::validator::ProxyCheckErrorType::RequestFailed => "request_failed",
        crate::validator::ProxyCheckErrorType::BadStatus => "bad_status",
        crate::validator::ProxyCheckErrorType::BodyReadFailed => "body_read_failed",
        crate::validator::ProxyCheckErrorType::ClientBuildFailed => "client_build_failed",
        crate::validator::ProxyCheckErrorType::InvalidProxyUrl => "invalid_proxy_url",
    }
}

fn count_by_source(proxies: &[Proxy]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for proxy in proxies {
        increment_source_count(&mut counts, proxy);
    }
    counts
}

fn increment_source_count(counts: &mut HashMap<String, usize>, proxy: &Proxy) {
    if let Some(source) = proxy.source.as_deref() {
        *counts.entry(source.to_string()).or_insert(0) += 1;
    }
}

fn apply_source_quality_counts(
    reports: &mut [FetcherRunReport],
    unique_by_source: &HashMap<String, usize>,
    validated_by_source: &HashMap<String, usize>,
    stored_by_source: &HashMap<String, usize>,
) {
    for report in reports {
        report.set_quality_counts(
            unique_by_source.get(&report.id).copied().unwrap_or(0),
            validated_by_source.get(&report.id).copied().unwrap_or(0),
            stored_by_source.get(&report.id).copied().unwrap_or(0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetcher::base::FetcherCircuitState;
    use chrono::{Duration, Utc};
    use tokio::sync::mpsc;

    struct TestFetcher;

    #[async_trait::async_trait]
    impl Fetcher for TestFetcher {
        fn name(&self) -> &str {
            "TestFetcher"
        }

        async fn fetch(&self) -> Vec<Proxy> {
            Vec::new()
        }
    }

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
            unique: 0,
            validated: 0,
            stored: 0,
            validation_survival_rate: None,
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

    #[test]
    fn automatic_refresh_skips_low_survival_rate_fetcher() {
        let now = Utc::now();
        let mut report = FetcherRunReport {
            id: "low-quality-source".into(),
            name: "Low Quality Source".into(),
            status: crate::fetcher::base::FetcherRunStatus::Success,
            fetched: 100,
            parsed: 100,
            unique: 100,
            validated: 5,
            stored: 3,
            validation_survival_rate: Some(0.05), // 5% survival rate
            error: None,
            circuit_state: FetcherCircuitState::Closed,
            consecutive_failures: 0,
            last_error: None,
            last_attempt_at: Some(now),
            last_success_at: Some(now),
            opened_at: None,
            next_probe_at: None,
            action: None,
            started_at: None,
            finished_at: None,
            duration_ms: None,
        };

        // Should skip because survival rate < 10%
        assert!(should_skip_fetcher_for_run(false, Some(&report), now));
        // Manual refresh should still proceed
        assert!(!should_skip_fetcher_for_run(true, Some(&report), now));

        // A source with 15% survival rate should NOT be skipped
        report.validation_survival_rate = Some(0.15);
        assert!(!should_skip_fetcher_for_run(false, Some(&report), now));

        // A source with no unique proxies (unique=0) should not be skipped
        // even with low survival rate (division-by-zero guard)
        report.unique = 0;
        report.validation_survival_rate = Some(0.05);
        assert!(!should_skip_fetcher_for_run(false, Some(&report), now));
    }

    #[test]
    fn applies_source_quality_counts_to_matching_reports() {
        let mut reports = vec![
            FetcherRunReport {
                id: "source-a".into(),
                name: "Source A".into(),
                ..FetcherRunReport::never_run(&TestFetcher)
            },
            FetcherRunReport {
                id: "source-b".into(),
                name: "Source B".into(),
                ..FetcherRunReport::never_run(&TestFetcher)
            },
        ];
        let unique_by_source = HashMap::from([("source-a".to_string(), 3)]);
        let validated_by_source = HashMap::from([("source-a".to_string(), 2)]);
        let stored_by_source = HashMap::from([("source-a".to_string(), 1)]);

        apply_source_quality_counts(
            &mut reports,
            &unique_by_source,
            &validated_by_source,
            &stored_by_source,
        );

        assert_eq!(reports[0].unique, 3);
        assert_eq!(reports[0].validated, 2);
        assert_eq!(reports[0].stored, 1);
        assert_eq!(reports[0].validation_survival_rate, Some(2.0 / 3.0));
        assert_eq!(reports[1].unique, 0);
        assert_eq!(reports[1].validation_survival_rate, None);
    }
}
