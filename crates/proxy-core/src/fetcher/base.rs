//! Abstract base trait for proxy source fetchers.

use crate::models::Proxy;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use std::time::Instant;

/// Consecutive unsuccessful fetcher attempts before opening the source circuit.
pub const FETCHER_CIRCUIT_FAILURE_THRESHOLD: u32 = 3;
/// Base cooldown for an open fetcher source circuit.
pub const FETCHER_CIRCUIT_COOLDOWN_SEC: i64 = 300;
/// Maximum cooldown for repeated failed probes.
pub const FETCHER_CIRCUIT_MAX_COOLDOWN_SEC: i64 = 3600;

/// Latest known status of one fetcher run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FetcherRunStatus {
    NeverRun,
    Success,
    Empty,
    Error,
    Skipped,
}

/// Source-level circuit state for a configured fetcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FetcherCircuitState {
    Closed,
    Open,
    HalfOpen,
}

/// What kind of fetcher action produced the latest report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FetcherRunAction {
    Fetched,
    SkippedOpen,
    HalfOpenProbe,
    ManualProbe,
}

/// Structured metadata for a fetcher run.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FetcherRunReport {
    pub id: String,
    pub name: String,
    pub status: FetcherRunStatus,
    pub fetched: usize,
    pub parsed: usize,
    pub unique: usize,
    pub validated: usize,
    pub stored: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_survival_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub circuit_state: FetcherCircuitState,
    pub consecutive_failures: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opened_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_probe_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<FetcherRunAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

impl FetcherRunReport {
    /// Create the initial report for a configured fetcher that has not run yet.
    pub fn never_run(fetcher: &dyn Fetcher) -> Self {
        Self {
            id: fetcher.id(),
            name: fetcher.name().to_string(),
            status: FetcherRunStatus::NeverRun,
            fetched: 0,
            parsed: 0,
            unique: 0,
            validated: 0,
            stored: 0,
            validation_survival_rate: None,
            error: None,
            circuit_state: FetcherCircuitState::Closed,
            consecutive_failures: 0,
            last_error: None,
            last_attempt_at: None,
            last_success_at: None,
            opened_at: None,
            next_probe_at: None,
            action: None,
            started_at: None,
            finished_at: None,
            duration_ms: None,
        }
    }

    /// Create a completed report from raw/parsed counts and an optional error.
    pub fn completed(
        fetcher: &dyn Fetcher,
        started_at: DateTime<Utc>,
        started: Instant,
        fetched: usize,
        parsed: usize,
        error: Option<String>,
    ) -> Self {
        Self::completed_for(
            fetcher.id(),
            fetcher.name().to_string(),
            started_at,
            started,
            fetched,
            parsed,
            error,
        )
    }

    /// Create a completed report from explicit fetcher identity fields.
    pub fn completed_for(
        id: String,
        name: String,
        started_at: DateTime<Utc>,
        started: Instant,
        fetched: usize,
        parsed: usize,
        error: Option<String>,
    ) -> Self {
        let status = if error.is_some() {
            FetcherRunStatus::Error
        } else if parsed == 0 {
            FetcherRunStatus::Empty
        } else {
            FetcherRunStatus::Success
        };
        let finished_at = Utc::now();

        Self {
            id,
            name,
            status,
            fetched,
            parsed,
            unique: 0,
            validated: 0,
            stored: 0,
            validation_survival_rate: None,
            error,
            circuit_state: FetcherCircuitState::Closed,
            consecutive_failures: 0,
            last_error: None,
            last_attempt_at: None,
            last_success_at: (status == FetcherRunStatus::Success).then_some(finished_at),
            opened_at: None,
            next_probe_at: None,
            action: Some(FetcherRunAction::Fetched),
            started_at: Some(started_at),
            finished_at: Some(finished_at),
            duration_ms: Some(started.elapsed().as_millis() as u64),
        }
    }

    /// Return true when automatic refresh should skip this fetcher.
    pub fn should_skip_automatic_at(&self, now: DateTime<Utc>) -> bool {
        self.circuit_state == FetcherCircuitState::Open
            && self.next_probe_at.is_none_or(|deadline| now < deadline)
    }

    /// Return the visible circuit state at a point in time.
    pub fn effective_circuit_state_at(&self, now: DateTime<Utc>) -> FetcherCircuitState {
        if self.circuit_state == FetcherCircuitState::Open
            && self.next_probe_at.is_some_and(|deadline| now >= deadline)
        {
            return FetcherCircuitState::HalfOpen;
        }
        self.circuit_state
    }

    /// Return a copy with time-derived open circuits shown as half-open.
    pub fn with_effective_circuit_state(mut self, now: DateTime<Utc>) -> Self {
        self.circuit_state = self.effective_circuit_state_at(now);
        self
    }

    /// Attach post-fetch source quality counts from the scheduler pipeline.
    pub fn with_quality_counts(mut self, unique: usize, validated: usize, stored: usize) -> Self {
        self.set_quality_counts(unique, validated, stored);
        self
    }

    /// Update post-fetch source quality counts in place.
    pub fn set_quality_counts(&mut self, unique: usize, validated: usize, stored: usize) {
        self.unique = unique;
        self.validated = validated;
        self.stored = stored;
        self.validation_survival_rate = validation_survival_rate(unique, validated);
    }

    /// Build a skipped report for an automatic refresh blocked by an open source circuit.
    pub fn skipped_open(
        fetcher: &dyn Fetcher,
        previous: &FetcherRunReport,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: fetcher.id(),
            name: fetcher.name().to_string(),
            status: FetcherRunStatus::Skipped,
            fetched: 0,
            parsed: 0,
            unique: 0,
            validated: 0,
            stored: 0,
            validation_survival_rate: None,
            error: previous
                .last_error
                .clone()
                .or_else(|| previous.error.clone())
                .map(|err| format!("source circuit open: {err}")),
            circuit_state: FetcherCircuitState::Open,
            consecutive_failures: previous.consecutive_failures,
            last_error: previous
                .last_error
                .clone()
                .or_else(|| previous.error.clone()),
            last_attempt_at: previous.last_attempt_at,
            last_success_at: previous.last_success_at,
            opened_at: previous.opened_at,
            next_probe_at: previous.next_probe_at,
            action: Some(FetcherRunAction::SkippedOpen),
            started_at: Some(now),
            finished_at: Some(now),
            duration_ms: Some(0),
        }
    }

    /// Apply source-level circuit transition rules to a completed fetch attempt.
    pub fn apply_circuit_transition(
        mut self,
        previous: Option<&FetcherRunReport>,
        manual_refresh: bool,
        now: DateTime<Utc>,
    ) -> Self {
        let previous_state = previous
            .map(|report| report.effective_circuit_state_at(now))
            .unwrap_or(FetcherCircuitState::Closed);
        let finished_at = self.finished_at.unwrap_or(now);
        let previous_failures = previous.map_or(0, |report| report.consecutive_failures);

        self.last_attempt_at = Some(finished_at);
        self.last_success_at = previous.and_then(|report| report.last_success_at);
        self.action = Some(match (manual_refresh, previous_state) {
            (true, FetcherCircuitState::Open | FetcherCircuitState::HalfOpen) => {
                FetcherRunAction::ManualProbe
            }
            (false, FetcherCircuitState::HalfOpen) => FetcherRunAction::HalfOpenProbe,
            _ => FetcherRunAction::Fetched,
        });

        if self.status == FetcherRunStatus::Success {
            self.circuit_state = FetcherCircuitState::Closed;
            self.consecutive_failures = 0;
            self.last_error = None;
            self.last_success_at = Some(finished_at);
            self.opened_at = None;
            self.next_probe_at = None;
            return self;
        }

        if self.is_unsuccessful_attempt() {
            let failures = previous_failures.saturating_add(1);
            self.consecutive_failures = failures;
            self.last_error = self
                .attempt_error()
                .or_else(|| previous.and_then(|report| report.last_error.clone()));
            self.last_success_at = previous.and_then(|report| report.last_success_at);

            let should_open = failures >= FETCHER_CIRCUIT_FAILURE_THRESHOLD
                || previous_state == FetcherCircuitState::Open
                || previous_state == FetcherCircuitState::HalfOpen;

            if should_open {
                self.circuit_state = FetcherCircuitState::Open;
                self.opened_at = Some(now);
                self.next_probe_at =
                    Some(now + Duration::seconds(fetcher_circuit_cooldown_sec(failures)));
            } else {
                self.circuit_state = FetcherCircuitState::Closed;
                self.opened_at = None;
                self.next_probe_at = None;
            }
        }

        self
    }

    fn is_unsuccessful_attempt(&self) -> bool {
        matches!(
            self.status,
            FetcherRunStatus::Empty | FetcherRunStatus::Error
        )
    }

    fn attempt_error(&self) -> Option<String> {
        self.error.clone().or_else(|| {
            (self.status == FetcherRunStatus::Empty).then_some("empty fetch result".into())
        })
    }
}

fn fetcher_circuit_cooldown_sec(failures: u32) -> i64 {
    let multiplier = failures
        .saturating_sub(FETCHER_CIRCUIT_FAILURE_THRESHOLD)
        .saturating_add(1) as i64;
    (FETCHER_CIRCUIT_COOLDOWN_SEC * multiplier).min(FETCHER_CIRCUIT_MAX_COOLDOWN_SEC)
}

fn validation_survival_rate(unique: usize, validated: usize) -> Option<f64> {
    (unique > 0).then_some(validated as f64 / unique as f64)
}

/// Fetcher output plus the run report that produced it.
#[derive(Debug, Clone)]
pub struct FetcherOutput {
    pub proxies: Vec<Proxy>,
    pub report: FetcherRunReport,
}

impl FetcherOutput {
    /// Build output from a completed fetcher attempt.
    pub fn completed(
        fetcher: &dyn Fetcher,
        started_at: DateTime<Utc>,
        started: Instant,
        fetched: usize,
        proxies: Vec<Proxy>,
        error: Option<String>,
    ) -> Self {
        let parsed = proxies.len();
        let report =
            FetcherRunReport::completed(fetcher, started_at, started, fetched, parsed, error);
        Self { proxies, report }
    }
}

/// A fetcher scrapes a source and returns a list of raw proxies.
#[async_trait::async_trait]
pub trait Fetcher: Send + Sync {
    /// Stable machine-readable id for this configured fetcher.
    fn id(&self) -> String {
        self.name().to_ascii_lowercase()
    }

    /// Human-readable name of this fetcher (for logging).
    fn name(&self) -> &str;

    /// Whether this fetcher is enabled.
    fn enabled(&self) -> bool {
        true
    }

    /// Fetch proxies from this source. Returns an empty vec on error.
    async fn fetch(&self) -> Vec<Proxy>;

    /// Fetch proxies from this source and return structured run metadata.
    async fn fetch_with_report(&self) -> FetcherOutput {
        let started_at = Utc::now();
        let started = Instant::now();
        let proxies = self.fetch().await;
        let report = FetcherRunReport::completed_for(
            self.id(),
            self.name().to_string(),
            started_at,
            started,
            proxies.len(),
            proxies.len(),
            None,
        );
        FetcherOutput { proxies, report }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Protocol, Proxy};

    struct TestFetcher;

    #[async_trait::async_trait]
    impl Fetcher for TestFetcher {
        fn name(&self) -> &str {
            "TestFetcher"
        }

        async fn fetch(&self) -> Vec<Proxy> {
            vec![Proxy::new("1.2.3.4", 8080, Protocol::Http)]
        }
    }

    #[test]
    fn never_run_report_has_initial_status() {
        let fetcher = TestFetcher;
        let report = FetcherRunReport::never_run(&fetcher);
        assert_eq!(report.id, "testfetcher");
        assert_eq!(report.status, FetcherRunStatus::NeverRun);
        assert_eq!(report.fetched, 0);
        assert_eq!(report.parsed, 0);
        assert_eq!(report.unique, 0);
        assert_eq!(report.validated, 0);
        assert_eq!(report.stored, 0);
        assert_eq!(report.validation_survival_rate, None);
    }

    #[test]
    fn completed_report_classifies_status() {
        let fetcher = TestFetcher;
        let started_at = Utc::now();
        let started = Instant::now();

        let ok = FetcherRunReport::completed(&fetcher, started_at, started, 2, 1, None);
        assert_eq!(ok.status, FetcherRunStatus::Success);

        let empty = FetcherRunReport::completed(&fetcher, started_at, started, 0, 0, None);
        assert_eq!(empty.status, FetcherRunStatus::Empty);

        let error = FetcherRunReport::completed(
            &fetcher,
            started_at,
            started,
            0,
            0,
            Some("network failed".into()),
        );
        assert_eq!(error.status, FetcherRunStatus::Error);
    }

    #[test]
    fn quality_counts_compute_survival_rate_and_serialize() {
        let fetcher = TestFetcher;
        let report = FetcherRunReport::never_run(&fetcher).with_quality_counts(4, 2, 1);

        assert_eq!(report.unique, 4);
        assert_eq!(report.validated, 2);
        assert_eq!(report.stored, 1);
        assert_eq!(report.validation_survival_rate, Some(0.5));

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"unique\":4"));
        assert!(json.contains("\"validated\":2"));
        assert!(json.contains("\"stored\":1"));
        assert!(json.contains("\"validation_survival_rate\":0.5"));
    }

    #[test]
    fn circuit_opens_after_failure_threshold() {
        let fetcher = TestFetcher;
        let now = Utc::now();
        let started = Instant::now();

        let first =
            FetcherRunReport::completed(&fetcher, now, started, 0, 0, Some("timeout".into()))
                .apply_circuit_transition(None, false, now);
        assert_eq!(first.circuit_state, FetcherCircuitState::Closed);
        assert_eq!(first.consecutive_failures, 1);

        let second =
            FetcherRunReport::completed(&fetcher, now, started, 0, 0, Some("timeout".into()))
                .apply_circuit_transition(Some(&first), false, now);
        assert_eq!(second.circuit_state, FetcherCircuitState::Closed);
        assert_eq!(second.consecutive_failures, 2);

        let third =
            FetcherRunReport::completed(&fetcher, now, started, 0, 0, Some("timeout".into()))
                .apply_circuit_transition(Some(&second), false, now);
        assert_eq!(third.circuit_state, FetcherCircuitState::Open);
        assert_eq!(
            third.consecutive_failures,
            FETCHER_CIRCUIT_FAILURE_THRESHOLD
        );
        assert!(third.next_probe_at.is_some());
    }

    #[test]
    fn open_circuit_skips_automatic_without_incrementing_failures() {
        let fetcher = TestFetcher;
        let now = Utc::now();
        let previous = FetcherRunReport {
            circuit_state: FetcherCircuitState::Open,
            consecutive_failures: 4,
            last_error: Some("timeout".into()),
            opened_at: Some(now),
            next_probe_at: Some(now + Duration::seconds(60)),
            ..FetcherRunReport::never_run(&fetcher)
        };

        assert!(previous.should_skip_automatic_at(now));
        let skipped = FetcherRunReport::skipped_open(&fetcher, &previous, now);
        assert_eq!(skipped.status, FetcherRunStatus::Skipped);
        assert_eq!(skipped.action, Some(FetcherRunAction::SkippedOpen));
        assert_eq!(skipped.consecutive_failures, 4);
    }

    #[test]
    fn expired_open_circuit_is_half_open_and_closes_on_success() {
        let fetcher = TestFetcher;
        let now = Utc::now();
        let previous = FetcherRunReport {
            circuit_state: FetcherCircuitState::Open,
            consecutive_failures: 3,
            next_probe_at: Some(now - Duration::seconds(1)),
            ..FetcherRunReport::never_run(&fetcher)
        };

        assert_eq!(
            previous.effective_circuit_state_at(now),
            FetcherCircuitState::HalfOpen
        );

        let success = FetcherRunReport::completed(&fetcher, now, Instant::now(), 1, 1, None)
            .apply_circuit_transition(Some(&previous), false, now);

        assert_eq!(success.circuit_state, FetcherCircuitState::Closed);
        assert_eq!(success.consecutive_failures, 0);
        assert_eq!(success.action, Some(FetcherRunAction::HalfOpenProbe));
        assert!(success.next_probe_at.is_none());
    }

    #[test]
    fn half_open_failure_reopens_with_extended_cooldown() {
        let fetcher = TestFetcher;
        let now = Utc::now();
        let previous = FetcherRunReport {
            circuit_state: FetcherCircuitState::Open,
            consecutive_failures: 3,
            next_probe_at: Some(now - Duration::seconds(1)),
            ..FetcherRunReport::never_run(&fetcher)
        };

        let failed = FetcherRunReport::completed(
            &fetcher,
            now,
            Instant::now(),
            0,
            0,
            Some("timeout".into()),
        )
        .apply_circuit_transition(Some(&previous), false, now);

        assert_eq!(failed.circuit_state, FetcherCircuitState::Open);
        assert_eq!(failed.consecutive_failures, 4);
        assert_eq!(failed.action, Some(FetcherRunAction::HalfOpenProbe));
        assert_eq!(
            failed.next_probe_at,
            Some(now + Duration::seconds(FETCHER_CIRCUIT_COOLDOWN_SEC * 2))
        );
    }

    #[test]
    fn manual_refresh_probes_open_circuit_before_deadline() {
        let fetcher = TestFetcher;
        let now = Utc::now();
        let previous = FetcherRunReport {
            circuit_state: FetcherCircuitState::Open,
            consecutive_failures: 3,
            next_probe_at: Some(now + Duration::seconds(60)),
            ..FetcherRunReport::never_run(&fetcher)
        };

        let success = FetcherRunReport::completed(&fetcher, now, Instant::now(), 1, 1, None)
            .apply_circuit_transition(Some(&previous), true, now);

        assert_eq!(success.circuit_state, FetcherCircuitState::Closed);
        assert_eq!(success.action, Some(FetcherRunAction::ManualProbe));
        assert_eq!(success.consecutive_failures, 0);
    }
}
