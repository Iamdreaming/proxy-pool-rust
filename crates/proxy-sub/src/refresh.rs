//! Subscription refresh loop: periodically discovers, fetches, parses, and stores proxies.

use std::collections::HashSet;
use std::sync::Arc;

use proxy_core::config::SubscriptionConfig;
use proxy_core::store::ProxyStore;

use crate::convert::partition;
use crate::discover::{
    AggregatorConfig, AggregatorDiscover, AirportConfig, AirportDiscover, Discover,
    GitHubSearchConfig, GitHubSearchDiscover, StaticUrlDiscover, TelegramChannelConfig,
    TelegramConfig, TelegramDiscover,
};
use crate::models::SubscriptionProxy;
use crate::parser::parse_subscription;
use crate::pending::PendingStore;
use crate::source::SubscriptionSource;

/// Build discoverers from subscription configuration.
///
/// Creates up to three types of discoverers:
/// 1. `StaticUrlDiscover` — if `config.urls` is non-empty.
/// 2. `GitHubSearchDiscover` — if `config.github.enabled` is true.
/// 3. `AggregatorDiscover` — one per entry in `config.aggregators`.
pub fn build_discoverers(config: &SubscriptionConfig) -> Vec<Arc<dyn Discover>> {
    let mut discoverers: Vec<Arc<dyn Discover>> = Vec::new();

    // Static URLs
    if !config.urls.is_empty() {
        discoverers.push(Arc::new(StaticUrlDiscover::new(config.urls.clone())));
    }

    // GitHub search
    if config.github.enabled {
        let keywords = if config.github.keywords.is_empty() {
            vec!["clash free sub".to_string(), "v2ray free nodes".to_string()]
        } else {
            config.github.keywords.clone()
        };
        discoverers.push(Arc::new(GitHubSearchDiscover::new(GitHubSearchConfig {
            token: config.github.token.clone(),
            max_results: config.github.max_results,
            keywords,
            timeout_sec: config.fetch_timeout_sec,
        })));
    }

    // Aggregators
    for agg in &config.aggregators {
        discoverers.push(Arc::new(AggregatorDiscover::new(AggregatorConfig {
            url: agg.url.clone(),
            format: agg.format.clone(),
            timeout_sec: config.fetch_timeout_sec,
        })));
    }

    // Telegram channels
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
        discoverers.push(Arc::new(TelegramDiscover::new(TelegramConfig {
            channels,
            timeout_sec: config.fetch_timeout_sec,
        })));
    }

    // Airport auto-discovery (no store available here; only runtime registration).
    if config.airport.enabled {
        discoverers.push(Arc::new(AirportDiscover::new(
            AirportConfig {
                aggregator_sites: config.airport.aggregator_sites.clone(),
                cloudflare_worker_url: config.airport.cloudflare_worker_url.clone(),
                cloudflare_admin_auth: config.airport.cloudflare_admin_auth.clone(),
                cloudflare_email_domain: config.airport.cloudflare_email_domain.clone(),
                max_concurrent: config.airport.max_concurrent,
                timeout_sec: config.fetch_timeout_sec,
            },
            None,
        )));
    }

    discoverers
}

/// Run a single refresh cycle across all discoverers.
///
/// Steps:
/// 1. Call all discoverers to collect candidate URLs.
/// 2. Dedup the URL list.
/// 3. Evict expired cache entries.
/// 4. For each URL: fetch → parse → partition → store basics in ProxyStore, encrypted in PendingStore.
/// 5. Log summary (total_basic, total_encrypted, failed_urls).
pub async fn run_refresh_cycle(
    discoverers: &[Arc<dyn Discover>],
    source: &mut SubscriptionSource,
    store: &ProxyStore,
    pending: &PendingStore,
) {
    // 1. Collect URLs from all discoverers.
    let mut all_urls: Vec<String> = Vec::new();
    for disc in discoverers {
        tracing::info!(name = disc.name(), "running discoverer");
        let urls = disc.discover().await;
        tracing::info!(
            name = disc.name(),
            count = urls.len(),
            "discoverer finished"
        );
        all_urls.extend(urls);
    }

    // 2. Dedup URLs.
    let mut seen = HashSet::new();
    all_urls.retain(|url| seen.insert(url.clone()));
    tracing::info!(
        total_urls = all_urls.len(),
        "deduplicated subscription URLs"
    );

    // 3. Evict expired cache entries.
    source.evict_expired();

    // 4. Fetch, parse, partition, and store.
    let mut total_basic: usize = 0;
    let mut total_encrypted: usize = 0;
    let mut failed_urls: usize = 0;

    for url in &all_urls {
        let content = match source.fetch(url).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(url = %url, "fetch failed: {e}");
                failed_urls += 1;
                continue;
            }
        };

        let proxies: Vec<SubscriptionProxy> = parse_subscription(&content);
        if proxies.is_empty() {
            tracing::debug!(url = %url, "no proxies parsed from subscription");
            continue;
        }

        let (basics, encrypted) = partition(&proxies, url);

        // Store basic proxies into the pool.
        for proxy in &basics {
            if let Err(e) = store.add(proxy).await {
                tracing::warn!(url = %url, "failed to store basic proxy: {e}");
            }
        }
        total_basic += basics.len();

        // Store encrypted proxies into pending.
        if !encrypted.is_empty()
            && let Err(e) = pending.store_batch(&encrypted).await
        {
            tracing::warn!(url = %url, "failed to store encrypted proxies: {e}");
        }
        total_encrypted += encrypted.len();
    }

    // 5. Log summary.
    tracing::info!(
        total_basic,
        total_encrypted,
        failed_urls,
        "subscription refresh cycle completed"
    );
}

/// Run the subscription refresh loop indefinitely.
///
/// Each cycle calls [`run_refresh_cycle`], then sleeps for
/// `config.refresh_interval_sec` seconds before starting the next cycle.
pub async fn subscription_refresh_loop(
    config: SubscriptionConfig,
    discoverers: Vec<Arc<dyn Discover>>,
    mut source: SubscriptionSource,
    store: Arc<ProxyStore>,
    pending: Arc<PendingStore>,
) {
    let interval = std::time::Duration::from_secs(config.refresh_interval_sec);

    loop {
        tracing::info!("subscription refresh cycle starting");
        run_refresh_cycle(&discoverers, &mut source, &store, &pending).await;
        tracing::info!(
            sleep_secs = interval.as_secs(),
            "subscription refresh cycle sleeping"
        );
        tokio::time::sleep(interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxy_core::config::{
        AggregatorEntryConfig, GitHubDiscoverConfig, SubscriptionConfig, TelegramDiscoverConfig,
    };

    fn make_config(
        urls: Vec<String>,
        github_enabled: bool,
        aggregators: Vec<AggregatorEntryConfig>,
    ) -> SubscriptionConfig {
        SubscriptionConfig {
            urls,
            github: GitHubDiscoverConfig {
                enabled: github_enabled,
                token: None,
                max_results: 10,
                search_interval_sec: 86400,
                keywords: vec!["test keyword".to_string()],
            },
            aggregators,
            telegram: TelegramDiscoverConfig {
                enabled: false,
                channels: vec![],
            },
            airport: proxy_core::config::AirportDiscoverConfig::default(),
            checkin: proxy_core::config::CheckinConfig::default(),
            refresh_interval_sec: 3600,
            fetch_timeout_sec: 30,
            cache_ttl_sec: 1800,
        }
    }

    #[test]
    fn test_build_discoverers_static() {
        let config = make_config(vec!["https://example.com/sub".to_string()], false, vec![]);
        let discoverers = build_discoverers(&config);
        assert_eq!(discoverers.len(), 1);
        assert_eq!(discoverers[0].name(), "static_url");
    }

    #[test]
    fn test_build_discoverers_all() {
        let config = make_config(
            vec!["https://example.com/sub".to_string()],
            true,
            vec![AggregatorEntryConfig {
                url: "https://agg.example.com/list".to_string(),
                format: "text".to_string(),
                refresh_interval_sec: 43200,
            }],
        );
        let discoverers = build_discoverers(&config);
        assert_eq!(discoverers.len(), 3);
        assert_eq!(discoverers[0].name(), "static_url");
        assert_eq!(discoverers[1].name(), "github_search");
        assert_eq!(discoverers[2].name(), "aggregator");
    }
}
