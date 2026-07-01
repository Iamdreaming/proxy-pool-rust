//! Proxy source fetcher trait and built-in implementations.

pub mod base;
pub mod clarketm;
pub mod free_proxy_list;
pub mod geonode;
pub mod proxyscrape;
pub mod thespeedx;

pub use base::Fetcher;

use crate::config::FetchersConfig;
use std::sync::Arc;

/// Build the list of enabled fetchers from config.
pub fn build_fetchers(config: &FetchersConfig) -> Vec<Arc<dyn Fetcher>> {
    let mut fetchers: Vec<Arc<dyn Fetcher>> = Vec::new();
    if config.proxyscrape.enabled {
        fetchers.push(Arc::new(proxyscrape::ProxyScrapeFetcher::new("http")));
        fetchers.push(Arc::new(proxyscrape::ProxyScrapeFetcher::new("socks5")));
    }
    if config.thespeedx.enabled {
        fetchers.push(Arc::new(thespeedx::TheSpeedXFetcher::new("http")));
        fetchers.push(Arc::new(thespeedx::TheSpeedXFetcher::new("socks5")));
    }
    if config.free_proxy_list.enabled {
        fetchers.push(Arc::new(free_proxy_list::FreeProxyListFetcher::new()));
    }
    if config.clarketm.enabled {
        fetchers.push(Arc::new(clarketm::ClarketmFetcher::new("http")));
    }
    if config.geonode.enabled {
        fetchers.push(Arc::new(geonode::GeoNodeFetcher::new()));
    }
    fetchers
}
