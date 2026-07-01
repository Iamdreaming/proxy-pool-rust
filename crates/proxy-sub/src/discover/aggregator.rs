//! Aggregator project discoverer: fetches a URL list from an aggregator service.
//!
//! Supports three subscription-list formats:
//!
//! - **text** — one URL per line, skipping blanks and `#`-comments.
//! - **json** — a JSON array of `{ "url": "..." }` objects or plain strings.
//! - **yaml** — a YAML document with a `subscriptions:` key containing strings
//!   or `{url: ...}` objects.

use crate::discover::Discover;

/// Configuration for [`AggregatorDiscover`].
#[derive(Debug, Clone)]
pub struct AggregatorConfig {
    /// URL of the aggregator list endpoint.
    pub url: String,
    /// Response format: `"text"`, `"json"`, or `"yaml"`.
    pub format: String,
    /// HTTP request timeout in seconds.
    pub timeout_sec: u64,
}

/// A discoverer that fetches a subscription URL list from an aggregator service.
pub struct AggregatorDiscover {
    config: AggregatorConfig,
    client: reqwest::Client,
}

impl AggregatorDiscover {
    /// Create a new aggregator discoverer with the given configuration.
    pub fn new(config: AggregatorConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_sec))
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("failed to build reqwest client for aggregator: {e}");
                reqwest::Client::new()
            });
        Self { config, client }
    }
}

#[async_trait::async_trait]
impl Discover for AggregatorDiscover {
    fn name(&self) -> &str {
        "aggregator"
    }

    async fn discover(&self) -> Vec<String> {
        let resp = match self.client.get(&self.config.url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(name = self.name(), url = %self.config.url, "fetch failed: {e}");
                return Vec::new();
            }
        };

        let text = match resp.text().await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(name = self.name(), url = %self.config.url, "read body failed: {e}");
                return Vec::new();
            }
        };

        match self.config.format.as_str() {
            "text" => parse_text_list(&text),
            "json" => parse_json_list(&text),
            "yaml" => parse_yaml_list(&text),
            other => {
                tracing::warn!(name = self.name(), format = other, "unknown format");
                Vec::new()
            }
        }
    }
}

/// Parse a text-format subscription list.
///
/// Keeps only lines starting with `http://` or `https://`. Blank lines and
/// lines starting with `#` are ignored.
fn parse_text_list(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.trim())
        .filter(|line| line.starts_with("http://") || line.starts_with("https://"))
        .map(String::from)
        .collect()
}

/// Parse a JSON-format subscription list.
///
/// Handles a JSON array containing either `{ "url": "..." }` objects or plain
/// string entries. Invalid entries are silently skipped.
fn parse_json_list(text: &str) -> Vec<String> {
    let arr: Vec<serde_json::Value> = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("JSON parse failed: {e}");
            return Vec::new();
        }
    };

    arr.iter()
        .filter_map(|item| match item {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(map) => {
                map.get("url").and_then(|v| v.as_str()).map(String::from)
            }
            _ => None,
        })
        .collect()
}

/// Parse a YAML-format subscription list.
///
/// Expects a top-level `subscriptions:` key whose value is a sequence of
/// either plain strings or `{url: ...}` mappings.
fn parse_yaml_list(text: &str) -> Vec<String> {
    let doc: serde_yaml::Value = match serde_yaml::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("YAML parse failed: {e}");
            return Vec::new();
        }
    };

    let subs = match doc.get("subscriptions").and_then(|v| v.as_sequence()) {
        Some(s) => s,
        None => return Vec::new(),
    };

    subs.iter()
        .filter_map(|item| match item {
            serde_yaml::Value::String(s) => Some(s.clone()),
            serde_yaml::Value::Mapping(map) => map
                .get(serde_yaml::Value::String("url".into()))
                .and_then(|v| v.as_str())
                .map(String::from),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_list() {
        let input = "\
# Free proxies
https://sub1.example.com/clash.yaml

http://sub2.example.com/v2ray.txt
https://sub3.example.com/sub.yaml
";
        let urls = parse_text_list(input);
        assert_eq!(urls.len(), 3);
        assert_eq!(urls[0], "https://sub1.example.com/clash.yaml");
        assert_eq!(urls[1], "http://sub2.example.com/v2ray.txt");
        assert_eq!(urls[2], "https://sub3.example.com/sub.yaml");
    }

    #[test]
    fn test_parse_json_list() {
        let input = r#"[
            {"url": "https://sub1.example.com/clash.yaml"},
            "https://sub2.example.com/v2ray.txt"
        ]"#;
        let urls = parse_json_list(input);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://sub1.example.com/clash.yaml");
        assert_eq!(urls[1], "https://sub2.example.com/v2ray.txt");
    }

    #[test]
    fn test_parse_yaml_list() {
        let input = "\
subscriptions:
  - https://sub1.example.com/clash.yaml
  - url: https://sub2.example.com/v2ray.txt
";
        let urls = parse_yaml_list(input);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://sub1.example.com/clash.yaml");
        assert_eq!(urls[1], "https://sub2.example.com/v2ray.txt");
    }
}
