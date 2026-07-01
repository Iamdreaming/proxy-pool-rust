//! ProxyScrape v2/v4 API fetcher.

use crate::fetcher::base::Fetcher;
use crate::models::{Protocol, Proxy};

/// Fetches proxies from the ProxyScrape API.
pub struct ProxyScrapeFetcher {
    protocol: String,
    timeout_secs: u64,
}

impl ProxyScrapeFetcher {
    pub fn new(protocol: &str) -> Self {
        Self {
            protocol: protocol.to_string(),
            timeout_secs: 15,
        }
    }
}

#[async_trait::async_trait]
impl Fetcher for ProxyScrapeFetcher {
    fn name(&self) -> &str {
        "ProxyScrape"
    }

    async fn fetch(&self) -> Vec<Proxy> {
        let url = "https://api.proxyscrape.com/v4/free-proxy-list/get";
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

        let resp = match client
            .get(url)
            .query(&[
                ("protocol", self.protocol.as_str()),
                ("timeout", "10000"),
                ("country", "all"),
                ("ssl", "all"),
                ("anonymity", "all"),
                ("format", "text"),
            ])
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("{}: fetch failed: {e}", self.name());
                return Vec::new();
            }
        };

        let text = match resp.text().await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("{}: read body failed: {e}", self.name());
                return Vec::new();
            }
        };

        let protocol = match Protocol::from_str_loose(&self.protocol) {
            Some(p) => p,
            None => return Vec::new(),
        };

        parse_text_list(&text, protocol, self.name())
    }
}

/// Parse a plain-text `host:port` list (one per line).
fn parse_text_list(text: &str, protocol: Protocol, source: &str) -> Vec<Proxy> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || !line.contains(':') {
                return None;
            }
            let (host, port_str) = line.rsplit_once(':')?;
            let port: u16 = port_str.parse().ok()?;
            if port == 0 {
                return None;
            }
            Some(Proxy {
                host: host.to_string(),
                port,
                protocol,
                source: Some(source.to_string()),
                ..Proxy::new(host, port, protocol)
            })
        })
        .collect()
}
