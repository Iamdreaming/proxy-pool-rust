//! In-memory content cache with TTL-based lazy eviction.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// In-memory cache for fetched subscription content.
///
/// Each entry stores the fetched text alongside an [`Instant`] recording
/// when it was cached. Entries that have exceeded the configured TTL are
/// lazily evicted on access (via [`get`](ContentCache::get)) or
/// proactively via [`evict_expired`](ContentCache::evict_expired).
///
/// This cache is **not** thread-safe; it is used behind `&mut self` in
/// [`SubscriptionSource`](crate::source::SubscriptionSource).
#[derive(Debug, Clone)]
pub struct ContentCache {
    /// TTL for cached entries.
    ttl: Duration,
    /// Map from URL to (content, insertion instant).
    store: HashMap<String, (String, Instant)>,
}

impl ContentCache {
    /// Create a new cache with the given TTL in seconds.
    pub fn new(ttl_sec: u64) -> Self {
        Self {
            ttl: Duration::from_secs(ttl_sec),
            store: HashMap::new(),
        }
    }

    /// Retrieve cached content for `url` if it has not expired.
    ///
    /// If the entry exists but has expired, it is removed (lazy eviction)
    /// and `None` is returned.
    pub fn get(&mut self, url: &str) -> Option<String> {
        if let Some((content, inserted)) = self.store.get(url)
            && inserted.elapsed() < self.ttl
        {
            return Some(content.clone());
        }
        // Entry missing or expired — remove if present and return None.
        self.store.remove(url);
        None
    }

    /// Store content for `url` with the current timestamp.
    pub fn put(&mut self, url: &str, content: &str) {
        self.store
            .insert(url.to_string(), (content.to_string(), Instant::now()));
    }

    /// Remove all expired entries from the cache.
    pub fn evict_expired(&mut self) {
        self.store
            .retain(|_url, (_, inserted)| inserted.elapsed() < self.ttl);
    }

    /// Number of entries currently in the cache (including potentially expired ones).
    pub fn len(&self) -> usize {
        self.store.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_put_get() {
        let mut cache = ContentCache::new(60);
        cache.put("https://example.com", "hello");
        assert_eq!(cache.get("https://example.com"), Some("hello".to_string()));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_missing_key() {
        let mut cache = ContentCache::new(60);
        assert_eq!(cache.get("https://missing.com"), None);
    }

    #[test]
    fn test_expired_entry_is_evicted_on_get() {
        // TTL of 0 seconds — entries expire immediately.
        let mut cache = ContentCache::new(0);
        cache.put("https://example.com", "stale");
        // Even a zero-duration TTL: Instant::now() to elapsed() is >= 0.
        // On most platforms, elapsed() will already be >= 0ns, so the
        // entry is considered expired and removed.
        assert_eq!(cache.get("https://example.com"), None);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_evict_expired() {
        let mut cache = ContentCache::new(0);
        cache.put("https://a.com", "a");
        cache.put("https://b.com", "b");
        // Both are immediately expired due to TTL=0.
        cache.evict_expired();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_len_and_is_empty() {
        let mut cache = ContentCache::new(60);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        cache.put("https://a.com", "a");
        assert!(!cache.is_empty());
        assert_eq!(cache.len(), 1);
    }
}
