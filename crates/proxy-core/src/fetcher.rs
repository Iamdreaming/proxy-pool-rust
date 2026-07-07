//! Proxy source fetcher trait and built-in implementations.

pub mod base;
pub mod clarketm;
pub mod free_proxy_list;
pub mod geonode;
pub mod proxyscrape;
pub mod public_lists;
pub mod thespeedx;

pub use base::Fetcher;

use crate::config::FetchersConfig;
use std::sync::Arc;

/// Build the list of enabled fetchers from config.
pub fn build_fetchers(config: &FetchersConfig) -> Vec<Arc<dyn Fetcher>> {
    let mut fetchers: Vec<Arc<dyn Fetcher>> = Vec::new();
    let github_mirror = config.github_mirror_prefix.as_deref();
    let mirror = config.mirror_prefix.as_deref();
    if config.proxyscrape.enabled {
        let mirror = if config.proxyscrape.use_mirror {
            mirror
        } else {
            None
        };
        fetchers.push(Arc::new(proxyscrape::ProxyScrapeFetcher::new(
            "http", mirror,
        )));
        fetchers.push(Arc::new(proxyscrape::ProxyScrapeFetcher::new(
            "https", mirror,
        )));
        fetchers.push(Arc::new(proxyscrape::ProxyScrapeFetcher::new(
            "socks5", mirror,
        )));
    }
    if config.thespeedx.enabled {
        fetchers.push(Arc::new(thespeedx::TheSpeedXFetcher::new(
            "http",
            github_mirror,
        )));
        fetchers.push(Arc::new(thespeedx::TheSpeedXFetcher::new(
            "socks5",
            github_mirror,
        )));
    }
    if config.free_proxy_list.enabled {
        let mirror = if config.free_proxy_list.use_mirror {
            mirror
        } else {
            None
        };
        fetchers.push(Arc::new(free_proxy_list::FreeProxyListFetcher::new(mirror)));
    }
    if config.clarketm.enabled {
        fetchers.push(Arc::new(clarketm::ClarketmFetcher::new(
            "http",
            github_mirror,
        )));
    }
    if config.geonode.enabled {
        let mirror = if config.geonode.use_mirror {
            mirror
        } else {
            None
        };
        fetchers.push(Arc::new(geonode::GeoNodeFetcher::new(mirror)));
    }
    if config.proxifly.enabled {
        let mirror = if config.proxifly.use_mirror {
            github_mirror
        } else {
            None
        };
        fetchers.push(Arc::new(public_lists::PublicListFetcher::proxifly(mirror)));
    }
    if config.databay.enabled {
        let mirror = if config.databay.use_mirror {
            github_mirror
        } else {
            None
        };
        fetchers.push(Arc::new(public_lists::PublicListFetcher::databay_http(
            mirror,
        )));
        fetchers.push(Arc::new(public_lists::PublicListFetcher::databay_socks4(
            mirror,
        )));
        fetchers.push(Arc::new(public_lists::PublicListFetcher::databay_socks5(
            mirror,
        )));
    }
    if config.iplocate.enabled {
        let mirror = if config.iplocate.use_mirror {
            github_mirror
        } else {
            None
        };
        fetchers.push(Arc::new(public_lists::PublicListFetcher::iplocate(mirror)));
    }
    if config.vpslab.enabled {
        let mirror = if config.vpslab.use_mirror {
            github_mirror
        } else {
            None
        };
        fetchers.push(Arc::new(public_lists::PublicListFetcher::vpslab_http(
            mirror,
        )));
        fetchers.push(Arc::new(public_lists::PublicListFetcher::vpslab_socks4(
            mirror,
        )));
        fetchers.push(Arc::new(public_lists::PublicListFetcher::vpslab_socks5(
            mirror,
        )));
    }
    if config.monosans.enabled {
        let mirror = if config.monosans.use_mirror {
            github_mirror
        } else {
            None
        };
        fetchers.push(Arc::new(public_lists::PublicListFetcher::monosans(mirror)));
    }
    fetchers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FetcherToggle;

    #[test]
    fn build_fetchers_includes_public_sources_by_default() {
        let ids: Vec<String> = build_fetchers(&FetchersConfig::default())
            .into_iter()
            .map(|fetcher| fetcher.id())
            .collect();

        for id in [
            "proxifly:all",
            "databay:http",
            "databay:socks4",
            "databay:socks5",
            "iplocate:all",
            "vpslab:http",
            "vpslab:socks4",
            "vpslab:socks5",
            "monosans:json",
        ] {
            assert!(ids.iter().any(|actual| actual == id), "missing {id}");
        }
    }

    #[test]
    fn build_fetchers_respects_public_source_toggles() {
        let config = FetchersConfig {
            proxifly: FetcherToggle {
                enabled: false,
                ..FetcherToggle::default()
            },
            databay: FetcherToggle {
                enabled: false,
                ..FetcherToggle::default()
            },
            iplocate: FetcherToggle {
                enabled: false,
                ..FetcherToggle::default()
            },
            vpslab: FetcherToggle {
                enabled: false,
                ..FetcherToggle::default()
            },
            monosans: FetcherToggle {
                enabled: false,
                ..FetcherToggle::default()
            },
            ..FetchersConfig::default()
        };
        let ids: Vec<String> = build_fetchers(&config)
            .into_iter()
            .map(|fetcher| fetcher.id())
            .collect();

        assert!(!ids.iter().any(|id| id.starts_with("proxifly:")));
        assert!(!ids.iter().any(|id| id.starts_with("databay:")));
        assert!(!ids.iter().any(|id| id.starts_with("iplocate:")));
        assert!(!ids.iter().any(|id| id.starts_with("vpslab:")));
        assert!(!ids.iter().any(|id| id.starts_with("monosans:")));
    }
}
