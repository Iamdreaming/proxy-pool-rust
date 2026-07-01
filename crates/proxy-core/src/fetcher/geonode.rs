//! GeoNode API fetcher.

use crate::fetcher::base::Fetcher;
use crate::models::{Protocol, Proxy};

const URL: &str = "https://proxylist.geonode.com/api/proxy-list";

/// Fetches proxies from the GeoNode API.
pub struct GeoNodeFetcher {
    timeout_secs: u64,
}

impl GeoNodeFetcher {
    pub fn new() -> Self {
        Self { timeout_secs: 15 }
    }
}

impl Default for GeoNodeFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Fetcher for GeoNodeFetcher {
    fn name(&self) -> &str {
        "GeoNode"
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

        let resp = match client
            .get(URL)
            .query(&[
                ("limit", "500"),
                ("page", "1"),
                ("sort_by", "lastChecked"),
                ("sort_type", "desc"),
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

        let data: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("{}: parse JSON failed: {e}", self.name());
                return Vec::new();
            }
        };

        let proxies = match data.get("data").and_then(|d| d.as_array()) {
            Some(arr) => arr,
            None => return Vec::new(),
        };

        proxies
            .iter()
            .filter_map(|item| {
                let host = item.get("ip")?.as_str()?.to_string();
                let port_str = item.get("port")?.as_str()?;
                let port: u16 = port_str.parse().ok()?;
                if port == 0 {
                    return None;
                }

                let proto_str = item
                    .get("protocol")
                    .and_then(|v| v.as_str())
                    .unwrap_or("http");
                let protocol = Protocol::from_str_loose(proto_str)?;

                Some(Proxy {
                    host: host.clone(),
                    port,
                    protocol,
                    source: Some(self.name().to_string()),
                    ..Proxy::new(host, port, protocol)
                })
            })
            .collect()
    }
}
