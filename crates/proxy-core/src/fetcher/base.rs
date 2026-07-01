//! Abstract base trait for proxy source fetchers.

use crate::models::Proxy;

/// A fetcher scrapes a source and returns a list of raw proxies.
#[async_trait::async_trait]
pub trait Fetcher: Send + Sync {
    /// Human-readable name of this fetcher (for logging).
    fn name(&self) -> &str;

    /// Whether this fetcher is enabled.
    fn enabled(&self) -> bool {
        true
    }

    /// Fetch proxies from this source. Returns an empty vec on error.
    async fn fetch(&self) -> Vec<Proxy>;
}
