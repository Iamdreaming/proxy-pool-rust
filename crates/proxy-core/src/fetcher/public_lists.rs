//! Public raw/JSON proxy list fetchers.

use crate::fetcher::base::{Fetcher, FetcherOutput};
use crate::models::{Protocol, Proxy};
use chrono::Utc;
use serde::Deserialize;
use std::time::Instant;

const PROXIFLY_ALL_URL: &str =
    "https://raw.githubusercontent.com/proxifly/free-proxy-list/main/proxies/all/data.txt";
const DATABAY_HTTP_URL: &str =
    "https://raw.githubusercontent.com/databay-labs/free-proxy-list/master/http.txt";
const DATABAY_SOCKS4_URL: &str =
    "https://raw.githubusercontent.com/databay-labs/free-proxy-list/master/socks4.txt";
const DATABAY_SOCKS5_URL: &str =
    "https://raw.githubusercontent.com/databay-labs/free-proxy-list/master/socks5.txt";
const IPLOCATE_ALL_URL: &str =
    "https://raw.githubusercontent.com/iplocate/free-proxy-list/main/all-proxies.txt";
const VPSLAB_HTTP_URL: &str =
    "https://raw.githubusercontent.com/VPSLabCloud/VPSLab-Free-Proxy-List/main/http_all.txt";
const VPSLAB_SOCKS4_URL: &str =
    "https://raw.githubusercontent.com/VPSLabCloud/VPSLab-Free-Proxy-List/main/socks4_all.txt";
const VPSLAB_SOCKS5_URL: &str =
    "https://raw.githubusercontent.com/VPSLabCloud/VPSLab-Free-Proxy-List/main/socks5_all.txt";
const MONOSANS_JSON_URL: &str =
    "https://raw.githubusercontent.com/monosans/proxy-list/main/proxies.json";

#[derive(Debug, Clone, Copy)]
enum PublicListParser {
    Text,
    MonosansJson,
}

/// Fetcher for public free-proxy raw list sources.
pub struct PublicListFetcher {
    id: &'static str,
    name: &'static str,
    url: &'static str,
    fallback_protocol: Option<Protocol>,
    parser: PublicListParser,
    timeout_secs: u64,
    mirror_prefix: Option<String>,
}

impl PublicListFetcher {
    fn new(
        id: &'static str,
        name: &'static str,
        url: &'static str,
        fallback_protocol: Option<Protocol>,
        parser: PublicListParser,
        mirror_prefix: Option<&str>,
    ) -> Self {
        Self {
            id,
            name,
            url,
            fallback_protocol,
            parser,
            timeout_secs: 30,
            mirror_prefix: mirror_prefix.map(ToString::to_string),
        }
    }

    /// Build the Proxifly all-protocol public list fetcher.
    pub fn proxifly(mirror_prefix: Option<&str>) -> Self {
        Self::new(
            "proxifly:all",
            "Proxifly",
            PROXIFLY_ALL_URL,
            None,
            PublicListParser::Text,
            mirror_prefix,
        )
    }

    /// Build the Databay HTTP public list fetcher.
    pub fn databay_http(mirror_prefix: Option<&str>) -> Self {
        Self::new(
            "databay:http",
            "Databay",
            DATABAY_HTTP_URL,
            Some(Protocol::Http),
            PublicListParser::Text,
            mirror_prefix,
        )
    }

    /// Build the Databay SOCKS4 public list fetcher.
    pub fn databay_socks4(mirror_prefix: Option<&str>) -> Self {
        Self::new(
            "databay:socks4",
            "Databay",
            DATABAY_SOCKS4_URL,
            Some(Protocol::Socks4),
            PublicListParser::Text,
            mirror_prefix,
        )
    }

    /// Build the Databay SOCKS5 public list fetcher.
    pub fn databay_socks5(mirror_prefix: Option<&str>) -> Self {
        Self::new(
            "databay:socks5",
            "Databay",
            DATABAY_SOCKS5_URL,
            Some(Protocol::Socks5),
            PublicListParser::Text,
            mirror_prefix,
        )
    }

    /// Build the IPLocate all-protocol public list fetcher.
    pub fn iplocate(mirror_prefix: Option<&str>) -> Self {
        Self::new(
            "iplocate:all",
            "IPLocate",
            IPLOCATE_ALL_URL,
            None,
            PublicListParser::Text,
            mirror_prefix,
        )
    }

    /// Build the VPSLab HTTP public list fetcher.
    pub fn vpslab_http(mirror_prefix: Option<&str>) -> Self {
        Self::new(
            "vpslab:http",
            "VPSLab",
            VPSLAB_HTTP_URL,
            Some(Protocol::Http),
            PublicListParser::Text,
            mirror_prefix,
        )
    }

    /// Build the VPSLab SOCKS4 public list fetcher.
    pub fn vpslab_socks4(mirror_prefix: Option<&str>) -> Self {
        Self::new(
            "vpslab:socks4",
            "VPSLab",
            VPSLAB_SOCKS4_URL,
            Some(Protocol::Socks4),
            PublicListParser::Text,
            mirror_prefix,
        )
    }

    /// Build the VPSLab SOCKS5 public list fetcher.
    pub fn vpslab_socks5(mirror_prefix: Option<&str>) -> Self {
        Self::new(
            "vpslab:socks5",
            "VPSLab",
            VPSLAB_SOCKS5_URL,
            Some(Protocol::Socks5),
            PublicListParser::Text,
            mirror_prefix,
        )
    }

    /// Build the Monosans JSON public list fetcher.
    pub fn monosans(mirror_prefix: Option<&str>) -> Self {
        Self::new(
            "monosans:json",
            "Monosans",
            MONOSANS_JSON_URL,
            None,
            PublicListParser::MonosansJson,
            mirror_prefix,
        )
    }

    fn source_url(&self) -> String {
        match &self.mirror_prefix {
            Some(prefix) => format!("{prefix}{}", self.url),
            None => self.url.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl Fetcher for PublicListFetcher {
    fn id(&self) -> String {
        self.id.to_string()
    }

    fn name(&self) -> &str {
        self.name
    }

    async fn fetch(&self) -> Vec<Proxy> {
        self.fetch_with_report().await.proxies
    }

    async fn fetch_with_report(&self) -> FetcherOutput {
        let started_at = Utc::now();
        let started = Instant::now();

        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("{}: build client failed: {e}", self.name());
                return FetcherOutput::completed(
                    self,
                    started_at,
                    started,
                    0,
                    Vec::new(),
                    Some(format!("build client failed: {e}")),
                );
            }
        };

        let url = self.source_url();
        let resp = match client.get(&url).send().await {
            Ok(r) => match r.error_for_status() {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("{}: fetch returned bad status: {e}", self.name());
                    return FetcherOutput::completed(
                        self,
                        started_at,
                        started,
                        0,
                        Vec::new(),
                        Some(format!("fetch returned bad status: {e}")),
                    );
                }
            },
            Err(e) => {
                tracing::warn!("{}: fetch failed: {e}", self.name());
                return FetcherOutput::completed(
                    self,
                    started_at,
                    started,
                    0,
                    Vec::new(),
                    Some(format!("fetch failed: {e}")),
                );
            }
        };

        let text = match resp.text().await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("{}: read body failed: {e}", self.name());
                return FetcherOutput::completed(
                    self,
                    started_at,
                    started,
                    0,
                    Vec::new(),
                    Some(format!("read body failed: {e}")),
                );
            }
        };

        let parsed = match self.parser {
            PublicListParser::Text => Ok((
                count_text_candidates(&text),
                parse_text_list(&text, self.fallback_protocol, self.name()),
            )),
            PublicListParser::MonosansJson => parse_monosans_json(&text, self.name()),
        };

        let (fetched, proxies) = match parsed {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!("{}: parse body failed: {e}", self.name());
                return FetcherOutput::completed(
                    self,
                    started_at,
                    started,
                    0,
                    Vec::new(),
                    Some(format!("parse body failed: {e}")),
                );
            }
        };

        tracing::info!("{}: fetched {} proxies", self.name(), proxies.len());
        FetcherOutput::completed(self, started_at, started, fetched, proxies, None)
    }
}

fn count_text_candidates(text: &str) -> usize {
    text.lines().filter(|line| is_candidate_line(line)).count()
}

fn parse_text_list(text: &str, fallback_protocol: Option<Protocol>, source: &str) -> Vec<Proxy> {
    text.lines()
        .filter_map(|line| parse_proxy_entry(line, fallback_protocol, source))
        .collect()
}

fn parse_proxy_entry(
    line: &str,
    fallback_protocol: Option<Protocol>,
    source: &str,
) -> Option<Proxy> {
    let line = line.trim();
    if !is_candidate_line(line) {
        return None;
    }

    let (protocol, address) = if let Some((scheme, address)) = line.split_once("://") {
        (Protocol::from_str_loose(scheme.trim())?, address.trim())
    } else {
        (fallback_protocol?, line)
    };

    let address = address
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(address)
        .rsplit('@')
        .next()
        .unwrap_or(address)
        .trim();
    let (host, port_str) = address.rsplit_once(':')?;
    let host = host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    if host.is_empty() {
        return None;
    }

    let port: u16 = port_str.trim().parse().ok()?;
    if port == 0 {
        return None;
    }

    Some(Proxy {
        host: host.clone(),
        port,
        protocol,
        source: Some(source.to_string()),
        ..Proxy::new(host, port, protocol)
    })
}

fn is_candidate_line(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty() && !line.starts_with('#') && line.contains(':')
}

fn parse_monosans_json(text: &str, source: &str) -> Result<(usize, Vec<Proxy>), String> {
    let rows: Vec<MonosansProxyRow> =
        serde_json::from_str(text).map_err(|e| format!("parse JSON failed: {e}"))?;
    let fetched = rows.len();
    let proxies = rows
        .into_iter()
        .filter_map(|row| row.into_proxy(source))
        .collect();
    Ok((fetched, proxies))
}

#[derive(Debug, Deserialize)]
struct MonosansProxyRow {
    protocol: String,
    host: String,
    port: MonosansPort,
}

impl MonosansProxyRow {
    fn into_proxy(self, source: &str) -> Option<Proxy> {
        let protocol = Protocol::from_str_loose(&self.protocol)?;
        let host = self.host.trim().to_string();
        if host.is_empty() {
            return None;
        }
        let port = self.port.to_u16()?;
        if port == 0 {
            return None;
        }

        Some(Proxy {
            host: host.clone(),
            port,
            protocol,
            source: Some(source.to_string()),
            ..Proxy::new(host, port, protocol)
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MonosansPort {
    Number(u64),
    String(String),
}

impl MonosansPort {
    fn to_u16(&self) -> Option<u16> {
        match self {
            MonosansPort::Number(port) => u16::try_from(*port).ok(),
            MonosansPort::String(port) => port.parse().ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text_list_accepts_plain_and_url_entries() {
        let text = "\
            1.2.3.4:8080\n\
            socks5://5.6.7.8:1080\n\
            https://9.9.9.9:443/path\n";

        let proxies = parse_text_list(text, Some(Protocol::Http), "TestSource");

        assert_eq!(proxies.len(), 3);
        assert_eq!(proxies[0].host, "1.2.3.4");
        assert_eq!(proxies[0].port, 8080);
        assert_eq!(proxies[0].protocol, Protocol::Http);
        assert_eq!(proxies[1].protocol, Protocol::Socks5);
        assert_eq!(proxies[2].protocol, Protocol::Https);
        assert_eq!(proxies[2].host, "9.9.9.9");
    }

    #[test]
    fn parse_text_list_ignores_comments_and_invalid_entries() {
        let text = "\
            # generated list\n\
            \n\
            missing-port\n\
            http://:8080\n\
            socks6://1.2.3.4:1080\n\
            1.2.3.4:0\n\
            1.2.3.4:70000\n\
            2.2.2.2:80\n";

        let proxies = parse_text_list(text, Some(Protocol::Http), "TestSource");

        assert_eq!(count_text_candidates(text), 5);
        assert_eq!(proxies.len(), 1);
        assert_eq!(proxies[0].host, "2.2.2.2");
        assert_eq!(proxies[0].port, 80);
    }

    #[test]
    fn parse_text_list_requires_fallback_for_plain_entries() {
        let text = "1.2.3.4:8080\nsocks4://5.6.7.8:1080\n";

        let proxies = parse_text_list(text, None, "TestSource");

        assert_eq!(proxies.len(), 1);
        assert_eq!(proxies[0].protocol, Protocol::Socks4);
    }

    #[test]
    fn parse_monosans_json_accepts_numeric_and_string_ports() {
        let text = r#"[
            {"protocol":"http","host":"1.2.3.4","port":8080},
            {"protocol":"socks5","host":"5.6.7.8","port":"1080"},
            {"protocol":"bad","host":"9.9.9.9","port":80},
            {"protocol":"http","host":"","port":80},
            {"protocol":"http","host":"2.2.2.2","port":70000}
        ]"#;

        let (fetched, proxies) = parse_monosans_json(text, "Monosans").unwrap();

        assert_eq!(fetched, 5);
        assert_eq!(proxies.len(), 2);
        assert_eq!(proxies[0].protocol, Protocol::Http);
        assert_eq!(proxies[1].protocol, Protocol::Socks5);
    }

    #[test]
    fn public_list_fetcher_ids_are_stable() {
        assert_eq!(PublicListFetcher::proxifly(None).id(), "proxifly:all");
        assert_eq!(
            PublicListFetcher::databay_socks5(None).id(),
            "databay:socks5"
        );
        assert_eq!(PublicListFetcher::iplocate(None).id(), "iplocate:all");
        assert_eq!(PublicListFetcher::vpslab_http(None).id(), "vpslab:http");
        assert_eq!(PublicListFetcher::monosans(None).id(), "monosans:json");
    }
}
