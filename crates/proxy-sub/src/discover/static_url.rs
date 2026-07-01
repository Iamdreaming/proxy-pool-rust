//! Static URL list discoverer: returns a pre-configured list of subscription URLs.

/// A discoverer that simply returns a static list of URLs from configuration.
///
/// Useful for subscription sources that are known ahead of time and do not
/// require runtime discovery (e.g. manually curated subscription links).
#[derive(Debug, Clone)]
pub struct StaticUrlDiscover {
    urls: Vec<String>,
}

impl StaticUrlDiscover {
    /// Create a new static URL discoverer from the given URL list.
    pub fn new(urls: Vec<String>) -> Self {
        Self { urls }
    }
}

#[async_trait::async_trait]
impl crate::discover::Discover for StaticUrlDiscover {
    fn name(&self) -> &str {
        "static_url"
    }

    async fn discover(&self) -> Vec<String> {
        self.urls.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::Discover;

    #[tokio::test]
    async fn test_static_url_with_urls() {
        let urls = vec![
            "https://example.com/sub1".to_string(),
            "https://example.com/sub2".to_string(),
        ];
        let disc = StaticUrlDiscover::new(urls.clone());
        assert_eq!(disc.name(), "static_url");
        let discovered = disc.discover().await;
        assert_eq!(discovered, urls);
    }

    #[tokio::test]
    async fn test_static_url_empty() {
        let disc = StaticUrlDiscover::new(Vec::new());
        let discovered = disc.discover().await;
        assert!(discovered.is_empty());
    }
}
