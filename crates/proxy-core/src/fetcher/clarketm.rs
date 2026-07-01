//! clarketm proxy list fetcher.

use crate::fetcher::base::Fetcher;
use crate::models::{Protocol, Proxy};

const URL: &str = "https://raw.githubusercontent.com/clarketm/proxy-list/master/proxy-list-raw.txt";

/// Fetches proxies from clarketm's GitHub list.
pub struct ClarketmFetcher {
    protocol: String,
    timeout_secs: u64,
}

impl ClarketmFetcher {
    pub fn new(protocol: &str) -> Self {
        Self {
            protocol: protocol.to_string(),
            timeout_secs: 15,
        }
    }
}

#[async_trait::async_trait]
impl Fetcher for ClarketmFetcher {
    fn name(&self) -> &str {
        "Clarketm"
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

        let resp = match client.get(URL).send().await {
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
                    source: Some(self.name().to_string()),
                    ..Proxy::new(host, port, protocol)
                })
            })
            .collect()
    }
}
