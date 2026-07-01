//! Subscription source: fetches subscription content with caching.

pub mod cache;

use std::time::Duration;

use anyhow::Result;
use cache::ContentCache;

/// A subscription source that fetches content from URLs with in-memory caching.
///
/// Wraps a [`reqwest::Client`] for HTTP GET requests and a [`ContentCache`]
/// for TTL-based caching of responses. On [`fetch`](SubscriptionSource::fetch),
/// the cache is checked first; if the content is missing or expired, an HTTP
/// GET is performed and the result is stored in the cache for subsequent calls.
pub struct SubscriptionSource {
    client: reqwest::Client,
    cache: ContentCache,
    timeout: Duration,
}

impl SubscriptionSource {
    /// Create a new subscription source.
    ///
    /// - `cache_ttl_sec`: time-to-live for cached responses (seconds).
    /// - `timeout_sec`: HTTP request timeout (seconds).
    pub fn new(cache_ttl_sec: u64, timeout_sec: u64) -> Self {
        let timeout = Duration::from_secs(timeout_sec);
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            cache: ContentCache::new(cache_ttl_sec),
            timeout,
        }
    }

    /// Fetch subscription content from `url`.
    ///
    /// Returns cached content if available and not expired; otherwise
    /// performs an HTTP GET, stores the result in the cache, and returns it.
    pub async fn fetch(&mut self, url: &str) -> Result<String> {
        if let Some(cached) = self.cache.get(url) {
            tracing::debug!("subscription source: cache hit for {}", url);
            return Ok(cached);
        }

        tracing::debug!(
            "subscription source: fetching {} (timeout={:?})",
            url,
            self.timeout
        );
        let response = self.client.get(url).send().await?;
        let content = response.text().await?;
        self.cache.put(url, &content);
        Ok(content)
    }

    /// Evict all expired entries from the cache.
    pub fn evict_expired(&mut self) {
        self.cache.evict_expired();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constructor() {
        let src = SubscriptionSource::new(300, 10);
        // No public field access needed — just verify construction succeeds.
        assert_eq!(src.timeout, Duration::from_secs(10));
    }
}
