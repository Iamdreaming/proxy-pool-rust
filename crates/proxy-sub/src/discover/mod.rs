//! Source discovery infrastructure: the [`Discover`] trait and built-in discoverers.

pub mod aggregator;
pub mod airport;
pub mod github_search;
pub mod static_url;
pub mod telegram;

pub use aggregator::{AggregatorConfig, AggregatorDiscover};
pub use airport::{AirportConfig, AirportDiscover};
pub use github_search::{GitHubSearchConfig, GitHubSearchDiscover};
pub use static_url::StaticUrlDiscover;
pub use telegram::{TelegramChannelConfig, TelegramConfig, TelegramDiscover};

/// A source discoverer: finds subscription URLs from a specific channel.
///
/// Implementations may query static config, GitHub search, aggregator
/// projects, or any other source. Each discoverer has a human-readable
/// name for logging and a single async method that returns discovered URLs.
#[async_trait::async_trait]
pub trait Discover: Send + Sync {
    /// Human-readable name for this discoverer (used in logs).
    fn name(&self) -> &str;

    /// Discover subscription URLs from this source.
    ///
    /// Returns a list of raw URL strings. Errors during discovery should
    /// be logged internally and omitted from the result — the caller
    /// receives only the URLs that were successfully discovered.
    async fn discover(&self) -> Vec<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trivial discoverer for testing the trait object pattern.
    struct NullDiscover;

    #[async_trait::async_trait]
    impl Discover for NullDiscover {
        fn name(&self) -> &str {
            "null"
        }

        async fn discover(&self) -> Vec<String> {
            Vec::new()
        }
    }

    #[tokio::test]
    async fn test_trait_object_dispatch() {
        let disc: Box<dyn Discover> = Box::new(NullDiscover);
        assert_eq!(disc.name(), "null");
        let urls = disc.discover().await;
        assert!(urls.is_empty());
    }
}
