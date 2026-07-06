//! Abstract base trait for proxy source fetchers.

use crate::models::Proxy;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::time::Instant;

/// Latest known status of one fetcher run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FetcherRunStatus {
    NeverRun,
    Success,
    Empty,
    Error,
}

/// Structured metadata for a fetcher run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FetcherRunReport {
    pub id: String,
    pub name: String,
    pub status: FetcherRunStatus,
    pub fetched: usize,
    pub parsed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
            error: None,
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

        Self {
            id,
            name,
            status,
            fetched,
            parsed,
            error,
            started_at: Some(started_at),
            finished_at: Some(Utc::now()),
            duration_ms: Some(started.elapsed().as_millis() as u64),
        }
    }
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
}
