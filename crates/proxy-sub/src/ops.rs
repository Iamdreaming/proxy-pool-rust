//! Operator-facing subscription source status and manual refresh support.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use chrono::{DateTime, Utc};
use proxy_core::config::SubscriptionConfig;
use proxy_core::source_origin::{CredibilityLevel, SourceOrigin};
use proxy_core::store::ProxyStore;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::convert::partition;
use crate::discover::{
    AggregatorConfig, AggregatorDiscover, AirportConfig, AirportDiscover, Discover,
    GitHubSearchConfig, GitHubSearchDiscover, TelegramChannelConfig, TelegramConfig,
    TelegramDiscover,
};
use crate::models::SubscriptionProxy;
use crate::parser::parse_subscription;
use crate::pending::PendingStore;
use crate::source::SubscriptionSource;

/// Public source kind used by API/MCP status responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionSourceKind {
    StaticUrl,
    GithubSearch,
    Aggregator,
    Telegram,
    Airport,
}

/// Stable, safe-to-display description of a configured subscription source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionSourceDescriptor {
    pub id: String,
    pub kind: SubscriptionSourceKind,
    pub label: String,
    pub enabled: bool,
    /// Origin credibility tag for this source.
    #[serde(default = "default_origin")]
    pub origin: SourceOrigin,
    /// Timestamp of the last successful refresh (used for credibility degradation).
    #[serde(default)]
    pub last_success_at: Option<DateTime<Utc>>,
    /// Consecutive refresh failures since last success.
    #[serde(default)]
    pub consecutive_failures: u32,
}

fn default_origin() -> SourceOrigin {
    SourceOrigin::Manual
}

/// Manual refresh mode. Preview is the safe default and performs no writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionRefreshMode {
    Preview,
    Apply,
}

impl SubscriptionRefreshMode {
    pub fn from_apply(apply: bool) -> Self {
        if apply { Self::Apply } else { Self::Preview }
    }

    pub fn applies(self) -> bool {
        matches!(self, Self::Apply)
    }
}

/// High-level outcome for one source refresh report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionRefreshOutcome {
    Ok,
    Partial,
    Empty,
    Failed,
}

/// Operator-facing recommendation for whether a previewed source should be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionApplyDecision {
    Apply,
    Review,
    Reject,
}

/// Source-level quality metrics derived from a subscription refresh report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionSourceQualityMetrics {
    pub fetch_success_rate: Option<f64>,
    pub supported_protocol_ratio: Option<f64>,
    pub unknown_node_ratio: Option<f64>,
    pub duplicate_node_ratio: Option<f64>,
    pub parsed_nodes_per_url: Option<f64>,
}

/// Human-readable source recommendation attached to refresh reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionApplyRecommendation {
    pub decision: SubscriptionApplyDecision,
    pub grade: u8,
    pub reasons: Vec<String>,
    pub metrics: SubscriptionSourceQualityMetrics,
}

/// Sanitized per-stage error captured during a refresh attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionSourceError {
    pub stage: String,
    pub url: Option<String>,
    pub message: String,
}

/// Structured report for one configured subscription source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSourceReport {
    pub source: SubscriptionSourceDescriptor,
    pub mode: SubscriptionRefreshMode,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub elapsed_ms: u64,
    pub outcome: SubscriptionRefreshOutcome,
    pub last_error: Option<String>,
    pub discovered_urls: usize,
    pub unique_urls: usize,
    pub duplicate_urls: usize,
    pub fetched_urls: usize,
    pub failed_urls: usize,
    pub parsed_nodes: usize,
    pub direct_nodes: usize,
    pub encrypted_nodes: usize,
    pub unknown_nodes: usize,
    pub duplicate_nodes: usize,
    pub stored_basic: usize,
    pub stored_encrypted: usize,
    pub protocol_counts: BTreeMap<String, usize>,
    pub errors: Vec<SubscriptionSourceError>,
    #[serde(default = "default_apply_recommendation")]
    pub recommendation: SubscriptionApplyRecommendation,
    /// Subscription metadata (traffic/expiry) parsed from response headers.
    #[serde(default)]
    pub metadata: Option<SubscriptionMeta>,
}

/// Parsed subscription-userinfo header data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionMeta {
    /// Bytes uploaded.
    pub upload: u64,
    /// Bytes downloaded.
    pub download: u64,
    /// Total bytes allowed.
    pub total: u64,
    /// Expiry timestamp (Unix seconds), if provided.
    pub expire: Option<i64>,
    /// Ratio of remaining traffic (0.0–1.0).
    pub remaining_ratio: f64,
    /// Days remaining until expiry, if known.
    pub remaining_days: Option<f64>,
    /// Composite health score (0.0–1.0) combining traffic and time.
    pub health: f64,
}

impl SubscriptionMeta {
    /// Parse a `subscription-userinfo` header value.
    ///
    /// Format: `upload=U; download=D; total=T; expire=E`
    /// where U/D/T are in bytes and E is a Unix timestamp.
    pub fn parse(header: &str) -> Option<Self> {
        let mut upload: Option<u64> = None;
        let mut download: Option<u64> = None;
        let mut total: Option<u64> = None;
        let mut expire: Option<i64> = None;

        for part in header.split(';') {
            let part = part.trim();
            if let Some((key, value)) = part.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "upload" => upload = value.parse().ok(),
                    "download" => download = value.parse().ok(),
                    "total" => total = value.parse().ok(),
                    "expire" => expire = value.parse().ok(),
                    _ => {}
                }
            }
        }

        let upload = upload?;
        let download = download?;
        let total = total?;

        let remaining_bytes = total.saturating_sub(upload + download);
        let remaining_ratio = if total > 0 {
            remaining_bytes as f64 / total as f64
        } else {
            0.0
        };

        let now_ts = Utc::now().timestamp();
        let remaining_days = expire.map(|exp| {
            let remaining_secs = (exp - now_ts).max(0);
            remaining_secs as f64 / 86400.0
        });

        // Health = geometric mean of traffic ratio and time ratio.
        // If no expiry, health = remaining_ratio.
        let health = match (remaining_days, expire) {
            (Some(days), Some(_)) if days > 0.0 => {
                let time_ratio = (days / 30.0).min(1.0); // normalize to 30-day window
                (remaining_ratio * time_ratio).sqrt()
            }
            _ => remaining_ratio,
        };

        Some(Self {
            upload,
            download,
            total,
            expire,
            remaining_ratio,
            remaining_days,
            health,
        })
    }

    /// Whether this subscription is effectively expired (no traffic or past expiry).
    pub fn is_expired(&self) -> bool {
        if self.remaining_ratio < 0.01 {
            return true;
        }
        if let Some(days) = self.remaining_days
            && days < 0.5
        {
            return true;
        }
        false
    }

    /// Whether this subscription is in low-health state (needs attention).
    pub fn is_low_health(&self) -> bool {
        self.remaining_ratio < 0.1 || self.remaining_days.is_some_and(|d| d < 3.0)
    }
}

/// Status for a configured source plus its latest report, if any.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSourceStatus {
    pub source: SubscriptionSourceDescriptor,
    pub latest_report: Option<SubscriptionSourceReport>,
}

/// Snapshot returned by API/MCP status surfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSourcesSnapshot {
    pub enabled: bool,
    pub source_count: usize,
    pub sources: Vec<SubscriptionSourceStatus>,
}

#[derive(Clone)]
pub struct SubscriptionOpsHandle {
    state: SubscriptionOpsState,
    source: Arc<Mutex<SubscriptionSource>>,
    store: Arc<ProxyStore>,
    pending: Arc<PendingStore>,
}

impl SubscriptionOpsHandle {
    pub fn new(
        config: SubscriptionConfig,
        store: Arc<ProxyStore>,
        pending: Arc<PendingStore>,
    ) -> Self {
        let state = SubscriptionOpsState::from_config(&config, Some(store.clone()));
        let source = Arc::new(Mutex::new(SubscriptionSource::new(
            config.cache_ttl_sec,
            config.fetch_timeout_sec,
        )));
        Self {
            state,
            source,
            store,
            pending,
        }
    }

    pub async fn status(&self) -> SubscriptionSourcesSnapshot {
        self.state.snapshot().await
    }

    pub async fn refresh_source(
        &self,
        source_id: &str,
        mode: SubscriptionRefreshMode,
    ) -> Result<Option<SubscriptionSourceReport>> {
        let Some(entry) = self.state.entry(source_id).await else {
            return Ok(None);
        };
        let report = {
            let mut source = self.source.lock().await;
            run_entry(&entry, &mut source, &self.store, &self.pending, mode).await
        };
        self.state.update_report(report.clone()).await;
        Ok(Some(report))
    }

    pub async fn refresh_all(
        &self,
        mode: SubscriptionRefreshMode,
    ) -> Vec<SubscriptionSourceReport> {
        let entries = self.state.entries().await;
        let mut reports = Vec::with_capacity(entries.len());
        let mut source = self.source.lock().await;
        for entry in entries {
            let report = run_entry(&entry, &mut source, &self.store, &self.pending, mode).await;
            self.state.update_report(report.clone()).await;
            reports.push(report);
        }
        reports
    }

    /// Perform check-in + renewal across all registered airport accounts.
    ///
    /// Loads the persisted accounts, POSTs each panel's `/user/checkin`
    /// endpoint, persists the result, and triggers a free-plan renewal when the
    /// subscription is low on traffic or near expiry. A failure for one site is
    /// logged and never blocks the others.
    pub async fn run_checkin(&self) {
        let store = self.store.clone();
        let accounts = crate::airport::load_airport_accounts(&store).await;
        if accounts.is_empty() {
            tracing::info!("airport check-in skipped: no registered accounts");
            return;
        }
        let client = reqwest::Client::new();
        for account in &accounts {
            let Some(token) = &account.token else {
                continue;
            };
            let result = crate::checkin::checkin(&account.domain, token, &client).await;
            if result.success {
                tracing::info!(domain = %account.domain, "airport check-in succeeded");
            } else {
                tracing::warn!(
                    domain = %account.domain,
                    msg = %result.message,
                    "airport check-in failed"
                );
            }
            if let Err(e) = crate::checkin::save_checkin_result(&store, &result).await {
                tracing::warn!(
                    domain = %account.domain,
                    error = %e,
                    "failed to persist airport check-in result"
                );
            }
            crate::checkin::renew_if_needed(account, None, &client).await;
        }
    }
}

#[derive(Clone)]
struct SubscriptionOpsState {
    inner: Arc<RwLock<SubscriptionOpsInner>>,
}

struct SubscriptionOpsInner {
    entries: Vec<SubscriptionSourceEntry>,
    reports: HashMap<String, SubscriptionSourceReport>,
}

impl SubscriptionOpsState {
    fn from_config(config: &SubscriptionConfig, store: Option<Arc<ProxyStore>>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(SubscriptionOpsInner {
                entries: entries_from_config(config, store),
                reports: HashMap::new(),
            })),
        }
    }

    async fn snapshot(&self) -> SubscriptionSourcesSnapshot {
        let inner = self.inner.read().await;
        let sources = inner
            .entries
            .iter()
            .map(|entry| SubscriptionSourceStatus {
                source: entry.descriptor.clone(),
                latest_report: inner.reports.get(&entry.descriptor.id).cloned(),
            })
            .collect::<Vec<_>>();
        SubscriptionSourcesSnapshot {
            enabled: !inner.entries.is_empty(),
            source_count: inner.entries.len(),
            sources,
        }
    }

    async fn entries(&self) -> Vec<SubscriptionSourceEntry> {
        self.inner.read().await.entries.clone()
    }

    async fn entry(&self, source_id: &str) -> Option<SubscriptionSourceEntry> {
        self.inner
            .read()
            .await
            .entries
            .iter()
            .find(|entry| entry.descriptor.id == source_id)
            .cloned()
    }

    async fn update_report(&self, report: SubscriptionSourceReport) {
        self.inner
            .write()
            .await
            .reports
            .insert(report.source.id.clone(), report);
    }
}

#[derive(Clone)]
struct SubscriptionSourceEntry {
    descriptor: SubscriptionSourceDescriptor,
    target: SubscriptionSourceTarget,
}

#[derive(Clone)]
enum SubscriptionSourceTarget {
    StaticUrl { url: String },
    Discoverer { discoverer: Arc<dyn Discover> },
}

impl SubscriptionSourceEntry {
    async fn discover_urls(&self) -> Vec<String> {
        match &self.target {
            SubscriptionSourceTarget::StaticUrl { url } => vec![url.clone()],
            SubscriptionSourceTarget::Discoverer { discoverer } => discoverer.discover().await,
        }
    }
}

pub async fn subscription_ops_loop(config: SubscriptionConfig, ops: SubscriptionOpsHandle) {
    let interval = std::time::Duration::from_secs(config.refresh_interval_sec);
    let checkin_interval = std::time::Duration::from_secs(config.checkin.interval_sec);
    let mut last_checkin: Option<Instant> = None;

    loop {
        tracing::info!("subscription refresh cycle starting");
        let reports = ops.refresh_all(SubscriptionRefreshMode::Apply).await;
        let total_basic: usize = reports.iter().map(|report| report.stored_basic).sum();
        let total_encrypted: usize = reports.iter().map(|report| report.stored_encrypted).sum();
        let failed_urls: usize = reports.iter().map(|report| report.failed_urls).sum();
        tracing::info!(
            source_count = reports.len(),
            total_basic,
            total_encrypted,
            failed_urls,
            "subscription refresh cycle completed"
        );

        // Check-in phase: runs on its own (independent) interval so it does not
        // couple to the refresh cycle cadence. Failures are isolated per-site.
        if config.checkin.enabled {
            let due = last_checkin
                .map(|t| t.elapsed() >= checkin_interval)
                .unwrap_or(true);
            if due {
                last_checkin = Some(Instant::now());
                ops.run_checkin().await;
            }
        }

        tracing::info!(
            sleep_secs = interval.as_secs(),
            "subscription refresh cycle sleeping"
        );
        tokio::time::sleep(interval).await;
    }
}

async fn run_entry(
    entry: &SubscriptionSourceEntry,
    source: &mut SubscriptionSource,
    store: &ProxyStore,
    pending: &PendingStore,
    mode: SubscriptionRefreshMode,
) -> SubscriptionSourceReport {
    let started_at = Utc::now();
    let timer = Instant::now();
    let mut report = empty_report(entry.descriptor.clone(), mode, started_at);

    let discovered = entry.discover_urls().await;
    report.discovered_urls = discovered.len();
    if discovered.is_empty() && matches!(&entry.target, SubscriptionSourceTarget::Discoverer { .. })
    {
        report.errors.push(SubscriptionSourceError {
            stage: "discover".into(),
            url: None,
            message: "no subscription URLs discovered".into(),
        });
    }

    let mut seen_urls = HashSet::new();
    let mut unique_urls = Vec::new();
    for url in discovered {
        if seen_urls.insert(url.clone()) {
            unique_urls.push(url);
        } else {
            report.duplicate_urls += 1;
        }
    }
    report.unique_urls = unique_urls.len();

    source.evict_expired();
    let mut seen_nodes = HashSet::new();
    let mut staged_basics = Vec::new();
    let mut staged_encrypted = Vec::new();

    for url in unique_urls {
        // Protocol direct links (vmess://, trojan://, etc.) are parsed directly
        // without fetching — they are already the node content.
        if is_protocol_direct_link(&url) {
            let proxies: Vec<SubscriptionProxy> = parse_subscription(&url);
            if !proxies.is_empty() {
                report.fetched_urls += 1;
                for proxy in &proxies {
                    *report
                        .protocol_counts
                        .entry(proxy.protocol_label().to_string())
                        .or_insert(0) += 1;
                    if !seen_nodes.insert(proxy.dedup_key()) {
                        report.duplicate_nodes += 1;
                    }
                    if matches!(proxy, SubscriptionProxy::Unknown { .. }) {
                        report.unknown_nodes += 1;
                    }
                }
                report.parsed_nodes += proxies.len();
                let (basics, encrypted) = partition(&proxies, &url);
                report.direct_nodes += basics.len();
                report.encrypted_nodes += encrypted.len();
                if mode.applies() {
                    staged_basics.extend(basics.into_iter().map(|p| (p, "direct-link".into())));
                    if !encrypted.is_empty() {
                        staged_encrypted.push((encrypted, "direct-link".into()));
                    }
                }
            }
            continue;
        }

        let display_url = redact_url(&url);
        let content = match source.fetch(&url).await {
            Ok(content) => {
                report.fetched_urls += 1;
                content
            }
            Err(e) => {
                report.failed_urls += 1;
                report.errors.push(SubscriptionSourceError {
                    stage: "fetch".into(),
                    url: Some(display_url),
                    message: sanitize_error_message(&e.to_string(), &url),
                });
                continue;
            }
        };

        let proxies: Vec<SubscriptionProxy> = parse_subscription(&content);
        if proxies.is_empty() {
            continue;
        }

        for proxy in &proxies {
            *report
                .protocol_counts
                .entry(proxy.protocol_label().to_string())
                .or_insert(0) += 1;
            if !seen_nodes.insert(proxy.dedup_key()) {
                report.duplicate_nodes += 1;
            }
            if matches!(proxy, SubscriptionProxy::Unknown { .. }) {
                report.unknown_nodes += 1;
            }
        }

        report.parsed_nodes += proxies.len();
        let (basics, encrypted) = partition(&proxies, &url);
        report.direct_nodes += basics.len();
        report.encrypted_nodes += encrypted.len();

        if mode.applies() {
            staged_basics.extend(basics.into_iter().map(|proxy| (proxy, display_url.clone())));
            if !encrypted.is_empty() {
                staged_encrypted.push((encrypted, display_url));
            }
        }
    }

    report.recommendation = recommend_apply(&report);

    if mode.applies() && !apply_reject_policy(&mut report) {
        for (proxy, display_url) in &staged_basics {
            match store.add(proxy).await {
                Ok(()) => report.stored_basic += 1,
                Err(e) => report.errors.push(SubscriptionSourceError {
                    stage: "store_basic".into(),
                    url: Some(display_url.clone()),
                    message: e.to_string(),
                }),
            }
        }

        for (encrypted, display_url) in &staged_encrypted {
            match pending.store_batch(encrypted).await {
                Ok(()) => report.stored_encrypted += encrypted.len(),
                Err(e) => report.errors.push(SubscriptionSourceError {
                    stage: "store_encrypted".into(),
                    url: Some(display_url.clone()),
                    message: e.to_string(),
                }),
            }
        }
    }

    report.finished_at = Utc::now();
    report.elapsed_ms = timer.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    report.outcome = report_outcome(&report);
    report.last_error = report.errors.first().map(|error| error.message.clone());

    // Update credibility tracking on the descriptor.
    if report.outcome == SubscriptionRefreshOutcome::Ok
        || report.outcome == SubscriptionRefreshOutcome::Partial
    {
        report.source.last_success_at = Some(report.finished_at);
        report.source.consecutive_failures = 0;
    } else {
        report.source.consecutive_failures = report.source.consecutive_failures.saturating_add(1);
    }

    report
}

fn empty_report(
    source: SubscriptionSourceDescriptor,
    mode: SubscriptionRefreshMode,
    started_at: DateTime<Utc>,
) -> SubscriptionSourceReport {
    SubscriptionSourceReport {
        source,
        mode,
        started_at,
        finished_at: started_at,
        elapsed_ms: 0,
        outcome: SubscriptionRefreshOutcome::Empty,
        last_error: None,
        discovered_urls: 0,
        unique_urls: 0,
        duplicate_urls: 0,
        fetched_urls: 0,
        failed_urls: 0,
        parsed_nodes: 0,
        direct_nodes: 0,
        encrypted_nodes: 0,
        unknown_nodes: 0,
        duplicate_nodes: 0,
        stored_basic: 0,
        stored_encrypted: 0,
        protocol_counts: BTreeMap::new(),
        errors: Vec::new(),
        recommendation: default_apply_recommendation(),
        metadata: None,
    }
}

fn report_outcome(report: &SubscriptionSourceReport) -> SubscriptionRefreshOutcome {
    if report.parsed_nodes == 0 && !report.errors.is_empty() {
        SubscriptionRefreshOutcome::Failed
    } else if !report.errors.is_empty() {
        SubscriptionRefreshOutcome::Partial
    } else if report.parsed_nodes == 0 {
        SubscriptionRefreshOutcome::Empty
    } else {
        SubscriptionRefreshOutcome::Ok
    }
}

fn default_apply_recommendation() -> SubscriptionApplyRecommendation {
    SubscriptionApplyRecommendation {
        decision: SubscriptionApplyDecision::Reject,
        grade: 0,
        reasons: vec!["no_preview_metrics".into()],
        metrics: SubscriptionSourceQualityMetrics {
            fetch_success_rate: None,
            supported_protocol_ratio: None,
            unknown_node_ratio: None,
            duplicate_node_ratio: None,
            parsed_nodes_per_url: None,
        },
    }
}

const MIN_SUPPORTED_NODES_FOR_NOISY_REVIEW: usize = 20;

fn recommend_apply(report: &SubscriptionSourceReport) -> SubscriptionApplyRecommendation {
    let metrics = source_quality_metrics(report);
    let supported_nodes = report.direct_nodes + report.encrypted_nodes;
    let mut reasons = Vec::new();
    let credibility_level = credibility_degradation(&report.source);

    // --- Credibility degradation (long-term, days-level) ---
    // This works alongside the circuit breaker (short-term, seconds-level).
    if let Some(level) = credibility_level {
        match level {
            CredibilityLevel::Expired => {
                reasons.push(format!(
                    "source_origin_{}_expired_{:.0}_days_past_2x_window",
                    report.source.origin,
                    days_since_last_success(&report.source).unwrap_or(0.0)
                ));
            }
            CredibilityLevel::Stale => {
                reasons.push(format!(
                    "source_origin_{}_stale_{:.0}_days_past_window",
                    report.source.origin,
                    days_since_last_success(&report.source).unwrap_or(0.0)
                ));
            }
            CredibilityLevel::Fresh => {}
        }
    }

    // --- Standard quality gate checks ---
    if report.unique_urls == 0 {
        reasons.push("no_subscription_urls_discovered".into());
    }
    if report.fetched_urls == 0 {
        reasons.push("no_urls_fetched".into());
    }
    if supported_nodes == 0 {
        reasons.push("no_supported_nodes".into());
    }
    if metrics.fetch_success_rate.is_some_and(|rate| rate < 0.10) {
        reasons.push("fetch_success_rate_below_10_percent".into());
    }
    if metrics
        .supported_protocol_ratio
        .is_some_and(|ratio| ratio < 0.10)
    {
        reasons.push("supported_protocol_ratio_below_10_percent".into());
    }
    if metrics.unknown_node_ratio.is_some_and(|ratio| ratio > 0.80)
        && supported_nodes < MIN_SUPPORTED_NODES_FOR_NOISY_REVIEW
    {
        reasons.push("unknown_node_ratio_above_80_percent".into());
    }
    if report.parsed_nodes >= 20
        && metrics
            .duplicate_node_ratio
            .is_some_and(|ratio| ratio > 0.95)
    {
        reasons.push("duplicate_node_ratio_above_95_percent".into());
    }

    if !reasons.is_empty() {
        return SubscriptionApplyRecommendation {
            decision: SubscriptionApplyDecision::Reject,
            grade: source_quality_grade(&metrics),
            reasons,
            metrics,
        };
    }

    let mut review_reasons = Vec::new();
    // Stale credibility forces Review even if quality metrics would allow Apply.
    if credibility_level == Some(CredibilityLevel::Stale) {
        review_reasons.push("source_credibility_stale_forces_review".into());
    }
    if metrics.fetch_success_rate.is_some_and(|rate| rate < 0.60) {
        review_reasons.push("fetch_success_rate_below_60_percent".into());
    }
    if report.parsed_nodes < 20 {
        review_reasons.push("parsed_nodes_below_20".into());
    }
    if metrics
        .supported_protocol_ratio
        .is_some_and(|ratio| ratio < 0.50)
    {
        review_reasons.push("supported_protocol_ratio_below_50_percent".into());
    }
    if metrics.unknown_node_ratio.is_some_and(|ratio| ratio > 0.40) {
        review_reasons.push("unknown_node_ratio_above_40_percent".into());
    }
    if metrics
        .duplicate_node_ratio
        .is_some_and(|ratio| ratio > 0.70)
    {
        review_reasons.push("duplicate_node_ratio_above_70_percent".into());
    }

    let decision = if review_reasons.is_empty() {
        reasons.push("source_meets_apply_thresholds".into());
        SubscriptionApplyDecision::Apply
    } else {
        reasons.extend(review_reasons);
        reasons.push("source_has_usable_nodes_but_needs_review".into());
        SubscriptionApplyDecision::Review
    };

    SubscriptionApplyRecommendation {
        decision,
        grade: source_quality_grade(&metrics),
        reasons,
        metrics,
    }
}

fn source_quality_metrics(report: &SubscriptionSourceReport) -> SubscriptionSourceQualityMetrics {
    let attempted_urls = report.fetched_urls + report.failed_urls;
    let supported_nodes = report.direct_nodes + report.encrypted_nodes;

    SubscriptionSourceQualityMetrics {
        fetch_success_rate: ratio(report.fetched_urls, attempted_urls),
        supported_protocol_ratio: ratio(supported_nodes, report.parsed_nodes),
        unknown_node_ratio: ratio(report.unknown_nodes, report.parsed_nodes),
        duplicate_node_ratio: ratio(report.duplicate_nodes, report.parsed_nodes),
        parsed_nodes_per_url: if report.fetched_urls == 0 {
            None
        } else {
            Some(report.parsed_nodes as f64 / report.fetched_urls as f64)
        },
    }
}

fn source_quality_grade(metrics: &SubscriptionSourceQualityMetrics) -> u8 {
    let fetch = metrics.fetch_success_rate.unwrap_or(0.0);
    let supported = metrics.supported_protocol_ratio.unwrap_or(0.0);
    let unknown_penalty = 1.0 - metrics.unknown_node_ratio.unwrap_or(1.0);
    let duplicate_penalty = 1.0 - metrics.duplicate_node_ratio.unwrap_or(1.0);
    let yield_score = metrics
        .parsed_nodes_per_url
        .map(|value| (value / 20.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    ((fetch * 30.0)
        + (supported * 30.0)
        + (unknown_penalty * 20.0)
        + (duplicate_penalty * 10.0)
        + (yield_score * 10.0))
        .round()
        .clamp(0.0, 100.0) as u8
}

fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        Some(numerator as f64 / denominator as f64)
    }
}

/// Compute credibility degradation level for a source based on its origin
/// and time since last successful refresh.
fn credibility_degradation(source: &SubscriptionSourceDescriptor) -> Option<CredibilityLevel> {
    let days = days_since_last_success(source)?;
    source.origin.degradation(days as u32)
}

/// Days elapsed since the last successful refresh. Returns `None` if never
/// refreshed successfully (treated as "day 0" for permanent origins, or a
/// large value for non-permanent origins to trigger immediate degradation).
fn days_since_last_success(source: &SubscriptionSourceDescriptor) -> Option<f64> {
    match source.last_success_at {
        Some(ts) => Some((Utc::now() - ts).num_seconds() as f64 / 86400.0),
        None => {
            // Never succeeded: permanent origins are fine, others are very stale.
            if source.origin.is_permanent() {
                None
            } else {
                // Use a large value to ensure degradation kicks in.
                Some(999.0)
            }
        }
    }
}

fn apply_reject_policy(report: &mut SubscriptionSourceReport) -> bool {
    if report.mode == SubscriptionRefreshMode::Apply
        && report.recommendation.decision == SubscriptionApplyDecision::Reject
    {
        report.errors.insert(
            0,
            SubscriptionSourceError {
                stage: "recommendation_policy".into(),
                url: None,
                message: "apply blocked because source recommendation is reject".into(),
            },
        );
        true
    } else {
        false
    }
}

fn entries_from_config(
    config: &SubscriptionConfig,
    store: Option<Arc<ProxyStore>>,
) -> Vec<SubscriptionSourceEntry> {
    let mut entries = Vec::new();

    for (idx, url) in config.urls.iter().enumerate() {
        let id = format!("static-url-{}", idx + 1);
        entries.push(SubscriptionSourceEntry {
            descriptor: SubscriptionSourceDescriptor {
                id,
                kind: SubscriptionSourceKind::StaticUrl,
                label: redact_url(url),
                enabled: true,
                origin: SourceOrigin::Manual,
                last_success_at: None,
                consecutive_failures: 0,
            },
            target: SubscriptionSourceTarget::StaticUrl { url: url.clone() },
        });
    }

    if config.github.enabled {
        let discoverer = Arc::new(GitHubSearchDiscover::new(GitHubSearchConfig {
            token: config.github.token.clone(),
            max_results: config.github.max_results,
            keywords: github_keywords(config),
            timeout_sec: config.fetch_timeout_sec,
        }));
        entries.push(SubscriptionSourceEntry {
            descriptor: SubscriptionSourceDescriptor {
                id: "github-search".into(),
                kind: SubscriptionSourceKind::GithubSearch,
                label: "GitHub search".into(),
                enabled: true,
                origin: SourceOrigin::GitHub,
                last_success_at: None,
                consecutive_failures: 0,
            },
            target: SubscriptionSourceTarget::Discoverer { discoverer },
        });
    }

    for (idx, aggregator) in config.aggregators.iter().enumerate() {
        let id = format!("aggregator-{}", idx + 1);
        let discoverer = Arc::new(AggregatorDiscover::new(AggregatorConfig {
            url: aggregator.url.clone(),
            format: aggregator.format.clone(),
            timeout_sec: config.fetch_timeout_sec,
        }));
        entries.push(SubscriptionSourceEntry {
            descriptor: SubscriptionSourceDescriptor {
                id,
                kind: SubscriptionSourceKind::Aggregator,
                label: redact_url(&aggregator.url),
                enabled: true,
                origin: SourceOrigin::Aggregator,
                last_success_at: None,
                consecutive_failures: 0,
            },
            target: SubscriptionSourceTarget::Discoverer { discoverer },
        });
    }

    if config.telegram.enabled {
        let channels: Vec<TelegramChannelConfig> = config
            .telegram
            .channels
            .iter()
            .map(|c| TelegramChannelConfig {
                name: c.name.clone(),
                pages: c.pages,
                include: c.include.clone(),
                exclude: c.exclude.clone(),
                enabled: c.enabled,
            })
            .collect();
        let discoverer = Arc::new(TelegramDiscover::new(TelegramConfig {
            channels,
            timeout_sec: config.fetch_timeout_sec,
        }));
        entries.push(SubscriptionSourceEntry {
            descriptor: SubscriptionSourceDescriptor {
                id: "telegram".into(),
                kind: SubscriptionSourceKind::Telegram,
                label: "Telegram channels".into(),
                enabled: true,
                origin: SourceOrigin::Telegram,
                last_success_at: None,
                consecutive_failures: 0,
            },
            target: SubscriptionSourceTarget::Discoverer { discoverer },
        });
    }

    if config.airport.enabled {
        let discoverer = Arc::new(AirportDiscover::new(
            AirportConfig {
                aggregator_sites: config.airport.aggregator_sites.clone(),
                cloudflare_worker_url: config.airport.cloudflare_worker_url.clone(),
                cloudflare_admin_auth: config.airport.cloudflare_admin_auth.clone(),
                cloudflare_email_domain: config.airport.cloudflare_email_domain.clone(),
                max_concurrent: config.airport.max_concurrent,
                timeout_sec: config.fetch_timeout_sec,
            },
            store.clone(),
        ));
        entries.push(SubscriptionSourceEntry {
            descriptor: SubscriptionSourceDescriptor {
                id: "airport".into(),
                kind: SubscriptionSourceKind::Airport,
                label: "Airport auto-registration".into(),
                enabled: true,
                origin: SourceOrigin::Airport,
                last_success_at: None,
                consecutive_failures: 0,
            },
            target: SubscriptionSourceTarget::Discoverer { discoverer },
        });
    }

    entries
}

fn github_keywords(config: &SubscriptionConfig) -> Vec<String> {
    if config.github.keywords.is_empty() {
        vec!["clash free sub".to_string(), "v2ray free nodes".to_string()]
    } else {
        config.github.keywords.clone()
    }
}

/// Returns `true` if the URL is a protocol direct link (e.g. `vmess://…`).
fn is_protocol_direct_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    crate::models::PROTOCOL_LINK_SCHEMES
        .iter()
        .any(|s| lower.starts_with(s))
}

fn sanitize_error_message(message: &str, raw_url: &str) -> String {
    message.replace(raw_url, &redact_url(raw_url))
}

fn redact_url(raw_url: &str) -> String {
    match url::Url::parse(raw_url) {
        Ok(mut parsed) => {
            let had_query = parsed.query().is_some();
            let had_fragment = parsed.fragment().is_some();
            parsed.set_query(None);
            parsed.set_fragment(None);
            let mut display = parsed.to_string();
            if had_query {
                display.push_str("?redacted");
            }
            if had_fragment {
                display.push_str("#redacted");
            }
            display
        }
        Err(_) => raw_url
            .split_once('?')
            .map(|(prefix, _)| format!("{prefix}?redacted"))
            .unwrap_or_else(|| raw_url.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxy_core::config::{AggregatorEntryConfig, GitHubDiscoverConfig, TelegramDiscoverConfig};
    use proxy_core::models::Protocol;

    fn config_with_sources() -> SubscriptionConfig {
        SubscriptionConfig {
            urls: vec![
                "https://example.com/sub?token=secret".into(),
                "https://example.org/plain".into(),
            ],
            github: GitHubDiscoverConfig {
                enabled: true,
                token: Some("github-token".into()),
                max_results: 5,
                search_interval_sec: 86400,
                keywords: vec![],
            },
            aggregators: vec![AggregatorEntryConfig {
                url: "https://agg.example.com/list.yaml#secret".into(),
                format: "yaml".into(),
                refresh_interval_sec: 43200,
            }],
            telegram: TelegramDiscoverConfig {
                enabled: false,
                channels: vec![],
            },
            refresh_interval_sec: 3600,
            fetch_timeout_sec: 10,
            cache_ttl_sec: 300,
            airport: proxy_core::config::AirportDiscoverConfig::default(),
            checkin: proxy_core::config::CheckinConfig::default(),
        }
    }

    fn test_descriptor() -> SubscriptionSourceDescriptor {
        SubscriptionSourceDescriptor {
            id: "static-url-1".into(),
            kind: SubscriptionSourceKind::StaticUrl,
            label: "https://example.com/sub".into(),
            enabled: true,
            origin: SourceOrigin::Manual,
            last_success_at: None,
            consecutive_failures: 0,
        }
    }

    #[tokio::test]
    async fn test_snapshot_empty_config() {
        let state = SubscriptionOpsState::from_config(&SubscriptionConfig::default(), None);
        let snapshot = state.snapshot().await;
        assert!(!snapshot.enabled);
        assert_eq!(snapshot.source_count, 0);
        assert!(snapshot.sources.is_empty());
    }

    #[tokio::test]
    async fn test_descriptors_from_config_are_stable_and_redacted() {
        let state = SubscriptionOpsState::from_config(&config_with_sources(), None);
        let snapshot = state.snapshot().await;
        assert!(snapshot.enabled);
        assert_eq!(snapshot.source_count, 4);
        assert_eq!(snapshot.sources[0].source.id, "static-url-1");
        assert_eq!(
            snapshot.sources[0].source.kind,
            SubscriptionSourceKind::StaticUrl
        );
        assert_eq!(
            snapshot.sources[0].source.label,
            "https://example.com/sub?redacted"
        );
        assert_eq!(snapshot.sources[2].source.id, "github-search");
        assert_eq!(snapshot.sources[3].source.id, "aggregator-1");
        assert_eq!(
            snapshot.sources[3].source.label,
            "https://agg.example.com/list.yaml#redacted"
        );
    }

    #[test]
    fn test_refresh_mode_from_apply_defaults_preview() {
        assert_eq!(
            SubscriptionRefreshMode::from_apply(false),
            SubscriptionRefreshMode::Preview
        );
        assert_eq!(
            SubscriptionRefreshMode::from_apply(true),
            SubscriptionRefreshMode::Apply
        );
        assert!(!SubscriptionRefreshMode::Preview.applies());
        assert!(SubscriptionRefreshMode::Apply.applies());
    }

    #[test]
    fn test_report_outcome_matrix() {
        let descriptor = test_descriptor();
        let mut report = empty_report(descriptor, SubscriptionRefreshMode::Preview, Utc::now());
        assert_eq!(report_outcome(&report), SubscriptionRefreshOutcome::Empty);

        report.parsed_nodes = 2;
        assert_eq!(report_outcome(&report), SubscriptionRefreshOutcome::Ok);

        report.errors.push(SubscriptionSourceError {
            stage: "fetch".into(),
            url: Some("https://example.com/sub".into()),
            message: "timeout".into(),
        });
        assert_eq!(report_outcome(&report), SubscriptionRefreshOutcome::Partial);

        report.parsed_nodes = 0;
        assert_eq!(report_outcome(&report), SubscriptionRefreshOutcome::Failed);
    }

    #[test]
    fn test_report_serialization_uses_snake_case_modes() {
        let descriptor = test_descriptor();
        let mut report = empty_report(descriptor, SubscriptionRefreshMode::Preview, Utc::now());
        report.protocol_counts.insert("basic".into(), 1);
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"mode\":\"preview\""));
        assert!(json.contains("\"outcome\":\"empty\""));
        assert!(json.contains("\"protocol_counts\":{\"basic\":1}"));
        assert!(json.contains("\"recommendation\""));
        assert!(json.contains("\"decision\":\"reject\""));
    }

    #[test]
    fn test_recommend_apply_when_source_meets_thresholds() {
        let descriptor = test_descriptor();
        let mut report = empty_report(descriptor, SubscriptionRefreshMode::Preview, Utc::now());
        report.unique_urls = 4;
        report.fetched_urls = 3;
        report.failed_urls = 1;
        report.parsed_nodes = 30;
        report.direct_nodes = 8;
        report.encrypted_nodes = 12;
        report.unknown_nodes = 5;
        report.duplicate_nodes = 3;

        let recommendation = recommend_apply(&report);

        assert_eq!(recommendation.decision, SubscriptionApplyDecision::Apply);
        assert!(recommendation.grade >= 60);
        assert!(
            recommendation
                .reasons
                .contains(&"source_meets_apply_thresholds".into())
        );
        assert_eq!(recommendation.metrics.fetch_success_rate, Some(0.75));
    }

    #[test]
    fn test_recommend_review_when_source_is_usable_but_small() {
        let descriptor = test_descriptor();
        let mut report = empty_report(descriptor, SubscriptionRefreshMode::Preview, Utc::now());
        report.unique_urls = 1;
        report.fetched_urls = 1;
        report.parsed_nodes = 5;
        report.direct_nodes = 2;
        report.encrypted_nodes = 3;

        let recommendation = recommend_apply(&report);

        assert_eq!(recommendation.decision, SubscriptionApplyDecision::Review);
        assert!(
            recommendation
                .reasons
                .contains(&"parsed_nodes_below_20".into())
        );
    }

    #[test]
    fn test_recommend_review_when_noisy_source_has_enough_supported_nodes() {
        let descriptor = test_descriptor();
        let mut report = empty_report(descriptor, SubscriptionRefreshMode::Preview, Utc::now());
        report.unique_urls = 1;
        report.fetched_urls = 1;
        report.parsed_nodes = 3726;
        report.encrypted_nodes = 722;
        report.unknown_nodes = 3004;
        report.duplicate_nodes = 3222;

        let recommendation = recommend_apply(&report);

        assert_eq!(recommendation.decision, SubscriptionApplyDecision::Review);
        assert!(
            !recommendation
                .reasons
                .contains(&"unknown_node_ratio_above_80_percent".into())
        );
        assert!(
            recommendation
                .reasons
                .contains(&"source_has_usable_nodes_but_needs_review".into())
        );
    }

    #[test]
    fn test_recommend_reject_when_no_supported_nodes() {
        let descriptor = test_descriptor();
        let mut report = empty_report(descriptor, SubscriptionRefreshMode::Preview, Utc::now());
        report.unique_urls = 1;
        report.fetched_urls = 1;
        report.parsed_nodes = 10;
        report.unknown_nodes = 10;

        let recommendation = recommend_apply(&report);

        assert_eq!(recommendation.decision, SubscriptionApplyDecision::Reject);
        assert!(
            recommendation
                .reasons
                .contains(&"no_supported_nodes".into())
        );
        assert!(
            recommendation
                .reasons
                .contains(&"unknown_node_ratio_above_80_percent".into())
        );
    }

    #[test]
    fn test_rejected_apply_policy_blocks_writes() {
        let descriptor = test_descriptor();
        let mut report = empty_report(descriptor, SubscriptionRefreshMode::Apply, Utc::now());
        report.unique_urls = 1;
        report.fetched_urls = 1;
        report.parsed_nodes = 10;
        report.unknown_nodes = 10;
        report.recommendation = recommend_apply(&report);

        assert!(apply_reject_policy(&mut report));
        assert_eq!(report.stored_basic, 0);
        assert_eq!(report.stored_encrypted, 0);
        assert_eq!(report.errors[0].stage, "recommendation_policy");
    }

    #[test]
    fn test_unknown_and_duplicate_node_counting_helpers() {
        let proxies = vec![
            SubscriptionProxy::Basic {
                host: "1.1.1.1".into(),
                port: 8080,
                protocol: Protocol::Http,
                username: None,
                password: None,
            },
            SubscriptionProxy::Basic {
                host: "1.1.1.1".into(),
                port: 8080,
                protocol: Protocol::Http,
                username: None,
                password: None,
            },
            SubscriptionProxy::Unknown {
                raw_config: "vless://secret".into(),
            },
        ];
        let mut seen = HashSet::new();
        let mut duplicate_nodes = 0;
        let mut unknown_nodes = 0;
        for proxy in &proxies {
            if !seen.insert(proxy.dedup_key()) {
                duplicate_nodes += 1;
            }
            if matches!(proxy, SubscriptionProxy::Unknown { .. }) {
                unknown_nodes += 1;
            }
        }
        assert_eq!(duplicate_nodes, 1);
        assert_eq!(unknown_nodes, 1);
    }
}
