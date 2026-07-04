//! TheSpeedX GitHub raw proxy list fetcher.

use crate::fetcher::base::Fetcher;
use crate::models::{Protocol, Proxy};

const HTTP_URL: &str = "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/http.txt";
const SOCKS5_URL: &str = "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/socks5.txt";

/// Fetches proxies from TheSpeedX GitHub lists.
pub struct TheSpeedXFetcher {
    protocol: String,
    timeout_secs: u64,
    mirror_prefix: Option<String>,
}

impl TheSpeedXFetcher {
    pub fn new(protocol: &str, mirror_prefix: Option<&str>) -> Self {
        Self {
            protocol: protocol.to_string(),
            timeout_secs: 30,
            mirror_prefix: mirror_prefix.map(|s| s.to_string()),
        }
    }

    fn url(&self) -> String {
        let raw = match self.protocol.as_str() {
            "socks5" => SOCKS5_URL,
            _ => HTTP_URL,
        };
        match &self.mirror_prefix {
            Some(prefix) => format!("{prefix}{raw}"),
            None => raw.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl Fetcher for TheSpeedXFetcher {
    fn name(&self) -> &str {
        "TheSpeedX"
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

        let proxies: Vec<Proxy> = text
            .lines()
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
            .collect();
        tracing::info!("{}: fetched {} proxies", self.name(), proxies.len());
        proxies
    }
}
