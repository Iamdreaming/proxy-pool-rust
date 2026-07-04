//! free-proxy-list.net HTML table scraper.

use crate::fetcher::base::Fetcher;
use crate::models::{Protocol, Proxy};

const URL: &str = "https://free-proxy-list.net/";

/// Scrapes the HTML table at free-proxy-list.net.
pub struct FreeProxyListFetcher {
    timeout_secs: u64,
    mirror_prefix: Option<String>,
}

impl FreeProxyListFetcher {
    pub fn new(mirror_prefix: Option<&str>) -> Self {
        Self {
            timeout_secs: 30,
            mirror_prefix: mirror_prefix.map(|s| s.to_string()),
        }
    }

    fn url(&self) -> String {
        match &self.mirror_prefix {
            Some(prefix) => format!("{prefix}{URL}"),
            None => URL.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl Fetcher for FreeProxyListFetcher {
    fn name(&self) -> &str {
        "FreeProxyList"
    }

    async fn fetch(&self) -> Vec<Proxy> {
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("{}: build client failed: {e}", self.name());
                return Vec::new();
            }
        };

        let url = self.url();
        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("{}: fetch failed: {e}", self.name());
                return Vec::new();
            }
        };

        let html = match resp.text().await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("{}: read body failed: {e}", self.name());
                return Vec::new();
            }
        };

        let proxies = parse_html_table(&html, self.name());
        tracing::info!("{}: fetched {} proxies", self.name(), proxies.len());
        proxies
    }
}

fn parse_html_table(html: &str, source: &str) -> Vec<Proxy> {
    let document = scraper::Html::parse_document(html);
    let selector = match scraper::Selector::parse("table#proxylisttable tbody tr") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    document
        .select(&selector)
        .filter_map(|row| {
            let cells: Vec<_> = row.select(&scraper::Selector::parse("td").ok()?).collect();
            if cells.len() < 6 {
                return None;
            }

            let host = cells[0].inner_html().trim().to_string();
            let port_binding = cells[1].inner_html();
            let port_str = port_binding.trim();
            let https_flag = cells[5].inner_html().trim().to_lowercase();

            let port: u16 = port_str.parse().ok()?;
            if port == 0 || host.is_empty() {
                return None;
            }

            let protocol = if https_flag == "yes" {
                Protocol::Https
            } else {
                Protocol::Http
            };

            Some(Proxy {
                host: host.clone(),
                port,
                protocol,
                source: Some(source.to_string()),
                ..Proxy::new(host, port, protocol)
            })
        })
        .collect()
}
