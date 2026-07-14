//! Telegram channel discoverer: crawls public `t.me/s/{channel}` pages.
//!
//! Telegram exposes a static HTML view of a channel's message history at
//! `https://t.me/s/{channel}`. This discoverer fetches those pages, parses
//! message text with the `scraper` crate, and extracts:
//!
//! - **Subscription API URLs** — e.g. `/api/v1/client/subscribe?token=…`,
//!   `/link/{id}?sub=`, or `/sub/{32-char-hash}`.
//! - **Direct protocol links** — `vmess://`, `trojan://`, `ss://`, `ssr://`,
//!   `vless://`, `hysteria2://`, `hysteria://`, `tuic://`, `snell://`,
//!   `anytls://`.
//!
//! Pagination is walked backwards via the `?before={post_id}` query parameter
//! found in the page's "load more" link. Link matching is done with string
//! methods and `url::Url` parsing — no regex dependency is used.

use std::collections::HashSet;

use crate::discover::Discover;
use scraper::{Html, Selector};

/// Configuration for a single Telegram channel to crawl.
#[derive(Debug, Clone)]
pub struct TelegramChannelConfig {
    /// Channel name (the `{channel}` segment in `t.me/s/{channel}`).
    pub name: String,
    /// Number of pages (history windows) to crawl per refresh. Defaults to 1.
    pub pages: u32,
    /// Substring(s) a discovered URL must contain to be kept.
    ///
    /// Comma-separated; empty means "include all". Matching is
    /// case-insensitive.
    pub include: String,
    /// Substring(s) a discovered URL must NOT contain to be kept.
    ///
    /// Comma-separated; empty means "exclude none". Matching is
    /// case-insensitive.
    pub exclude: String,
    /// Whether this channel is enabled.
    pub enabled: bool,
}

/// Top-level configuration for the Telegram discoverer.
#[derive(Debug, Clone)]
pub struct TelegramConfig {
    /// Channels to crawl.
    pub channels: Vec<TelegramChannelConfig>,
    /// Per-request HTTP timeout, in seconds.
    pub timeout_sec: u64,
}

/// A discoverer that crawls Telegram channel public pages for proxy links.
pub struct TelegramDiscover {
    config: TelegramConfig,
    client: reqwest::Client,
}

impl TelegramDiscover {
    /// Create a new Telegram discoverer with the given configuration.
    pub fn new(config: TelegramConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_sec))
            .user_agent("Mozilla/5.0 (compatible; proxy-pool-rust/1.0)")
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("failed to build reqwest client for telegram: {e}");
                reqwest::Client::new()
            });
        Self { config, client }
    }

    /// Crawl all configured, enabled channels and return discovered URLs.
    ///
    /// Network failures for one channel are logged and do not prevent
    /// crawling of other channels. Returned URLs are deduplicated.
    async fn crawl_all(&self) -> Vec<String> {
        let mut all = Vec::new();
        for channel in &self.config.channels {
            if !channel.enabled {
                continue;
            }
            let urls = self.crawl_channel(channel).await;
            if urls.is_empty() {
                tracing::debug!(
                    name = self.name(),
                    channel = %channel.name,
                    "no URLs discovered from channel"
                );
            } else {
                tracing::info!(
                    name = self.name(),
                    channel = %channel.name,
                    count = urls.len(),
                    "discovered URLs from channel"
                );
            }
            all.extend(urls);
        }

        let mut seen = HashSet::new();
        all.retain(|url| seen.insert(url.clone()));
        all
    }

    /// Crawl a single channel up to `channel.pages` pages, following pagination.
    async fn crawl_channel(&self, channel: &TelegramChannelConfig) -> Vec<String> {
        let mut urls = Vec::new();
        let mut next_before: Option<String> = None;
        let pages = channel.pages.max(1);

        for _ in 0..pages {
            let page_url = match &next_before {
                Some(before) => format!("https://t.me/s/{}?before={}", channel.name, before),
                None => format!("https://t.me/s/{}", channel.name),
            };

            let html = match self.fetch_html(&page_url).await {
                Some(h) => h,
                None => break,
            };

            urls.extend(extract_urls_from_html(&html));

            match find_next_before(&html) {
                Some(before) => next_before = Some(before),
                None => break,
            }
        }

        urls.into_iter()
            .filter(|url| passes_filter(url, &channel.include, &channel.exclude))
            .collect()
    }

    /// Fetch a page; returns `None` on any network or non-success error (logged).
    async fn fetch_html(&self, url: &str) -> Option<String> {
        let resp = match self.client.get(url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(name = self.name(), url = %url, "fetch failed: {e}");
                return None;
            }
        };
        if !resp.status().is_success() {
            tracing::warn!(
                name = self.name(),
                url = %url,
                status = %resp.status(),
                "unexpected status fetching channel page"
            );
            return None;
        }
        match resp.text().await {
            Ok(t) => Some(t),
            Err(e) => {
                tracing::warn!(name = self.name(), url = %url, "read body failed: {e}");
                None
            }
        }
    }
}

#[async_trait::async_trait]
impl Discover for TelegramDiscover {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn discover(&self) -> Vec<String> {
        self.crawl_all().await
    }
}

/// Protocol-link schemes we extract directly from message text.
const PROTOCOL_PREFIXES: &[&str] = &[
    "vmess://",
    "trojan://",
    "ss://",
    "ssr://",
    "vless://",
    "hysteria2://",
    "hysteria://",
    "tuic://",
    "snell://",
    "anytls://",
];

/// Extract subscription and protocol URLs from a `t.me/s` HTML page.
fn extract_urls_from_html(html: &str) -> Vec<String> {
    let doc = Html::parse_document(html);

    let msg_sel = match Selector::parse(".tgme_widget_message") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let text_sel = match Selector::parse(".tgme_widget_message_text") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    // Collect both message text blobs and raw <a> hrefs as extraction sources.
    let mut blobs: Vec<String> = Vec::new();
    for msg in doc.select(&msg_sel) {
        let text = match msg.select(&text_sel).next() {
            Some(el) => el.text().collect::<Vec<_>>().join(" "),
            None => msg.text().collect::<Vec<_>>().join(" "),
        };
        if !text.trim().is_empty() {
            blobs.push(text);
        }
    }

    let a_sel = match Selector::parse("a") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    for a in doc.select(&a_sel) {
        if let Some(href) = a.value().attr("href")
            && !href.is_empty()
        {
            blobs.push(href.to_string());
        }
    }

    let mut seen = HashSet::new();
    let mut urls = Vec::new();
    for blob in &blobs {
        extract_from_blob(blob, &mut seen, &mut urls);
    }
    urls
}

/// Classify and extract URLs from a single text/href blob.
fn extract_from_blob(blob: &str, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    // `#MANAGED-CONFIG` / `#订阅链接` comment lines carry a plain URL on the
    // same line; those URLs are not otherwise recognized as subscriptions.
    if blob.contains("#MANAGED-CONFIG") || blob.contains("#订阅链接") {
        for line in blob.split('\n') {
            if line.contains("#MANAGED-CONFIG") || line.contains("#订阅链接") {
                for token in tokenize(line) {
                    if is_http_url(token) {
                        push_unique(token.to_string(), seen, out);
                    }
                }
            }
        }
    }

    for token in tokenize(blob) {
        if looks_like_subscription(token) || is_protocol_link(token) {
            push_unique(token.to_string(), seen, out);
        }
    }
}

/// Split a blob into candidate tokens on whitespace and link-breaking punctuation.
fn tokenize(text: &str) -> Vec<&str> {
    text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '<' | '>' | '(' | ')' | '{' | '}' | '[' | ']' | ',' | '`' | '\'' | '|'
            )
    })
    .filter(|t| !t.is_empty())
    .collect()
}

/// Whether `s` is an `http://` or `https://` URL.
fn is_http_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Heuristically detect whether a token is a proxy subscription URL.
///
/// Matches three common shapes without regex:
/// - `/api/v1/client/subscribe?token=` with a 16–32 char alphanumeric token
/// - `/link/{id}?sub=` / `?mu=` / `?clash=`
/// - `/sub/{32-char alphanumeric hash}`
fn looks_like_subscription(url: &str) -> bool {
    if !is_http_url(url) {
        return false;
    }

    if let Some(idx) = url.find("subscribe?token=") {
        let token = &url[idx + "subscribe?token=".len()..];
        let token_val = token.split(|c| !is_ascii_alnum(c)).next().unwrap_or("");
        return (16..=32).contains(&token_val.len());
    }

    if url.contains("/link/") {
        return url.contains("?sub=") || url.contains("?mu=") || url.contains("?clash=");
    }

    if let Some(idx) = url.find("/sub/") {
        let rest = &url[idx + "/sub/".len()..];
        let hash = rest.split(|c| !is_ascii_alnum(c)).next().unwrap_or("");
        return hash.len() == 32;
    }

    false
}

/// Whether `token` is a direct protocol link (>= 10 chars after the scheme).
fn is_protocol_link(token: &str) -> bool {
    for prefix in PROTOCOL_PREFIXES {
        if let Some(stripped) = token.strip_prefix(prefix) {
            return stripped.len() >= 10;
        }
    }
    false
}

/// Find the `before=` post id for the next page's "load more" link.
fn find_next_before(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let a_sel = Selector::parse("a").ok()?;
    for a in doc.select(&a_sel) {
        if let Some(href) = a.value().attr("href")
            && let Some(before) = extract_query_param(href, "before")
        {
            return Some(before);
        }
    }
    None
}

/// Extract a single query parameter value from a URL/href string.
fn extract_query_param(href: &str, key: &str) -> Option<String> {
    let marker = format!("{}=", key);
    let idx = href.find(&marker)?;
    let rest = &href[idx + marker.len()..];
    let end = rest.find(['&', '#']).unwrap_or(rest.len());
    let value = &rest[..end];
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Whether a URL passes the channel's include/exclude substring filters.
///
/// - `include` is a comma-separated list; if non-empty, the URL must contain
///   at least one include substring (case-insensitive).
/// - `exclude` is a comma-separated list; if non-empty, the URL must not
///   contain any exclude substring (case-insensitive).
fn passes_filter(url: &str, include: &str, exclude: &str) -> bool {
    let url_lc = url.to_lowercase();

    if !include.is_empty() {
        let mut matched = false;
        for needle in include.split(',') {
            let needle = needle.trim().to_lowercase();
            if !needle.is_empty() && url_lc.contains(&needle) {
                matched = true;
                break;
            }
        }
        if !matched {
            return false;
        }
    }

    if !exclude.is_empty() {
        for needle in exclude.split(',') {
            let needle = needle.trim().to_lowercase();
            if !needle.is_empty() && url_lc.contains(&needle) {
                return false;
            }
        }
    }

    true
}

/// Append `url` to `out` unless it has already been seen.
fn push_unique(url: String, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    if seen.insert(url.clone()) {
        out.push(url);
    }
}

/// ASCII alphanumeric predicate used for token/segment extraction.
fn is_ascii_alnum(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sample `t.me/s` page with several distinct link shapes.
    const SAMPLE_HTML: &str = r#"
        <div class="tgme_widget_message">
          <div class="tgme_widget_message_text">Free sub: https://sub.example.com/link/abc123?sub=1 and vmess://eyJ2bG9uZ0Vub3VnaDEyMzQ1Njc4OTA=</div>
        </div>
        <div class="tgme_widget_message">
          <div class="tgme_widget_message_text">Panel https://panel.example.com/api/v1/client/subscribe?token=ABCDEF1234567890</div>
        </div>
        <div class="tgme_widget_message">
          <div class="tgme_widget_message_text">Hash sub https://x.example.com/sub/0123456789abcdef0123456789abcdef</div>
        </div>
        <div class="tgme_widget_message">
          <div class="tgme_widget_message_text">#MANAGED-CONFIG https://managed.example.com/clash.yaml</div>
        </div>
        <a href="https://t.me/s/proxy_list?before=99999">Load more</a>
    "#;

    #[test]
    fn test_extract_urls_from_sample_html() {
        let urls = extract_urls_from_html(SAMPLE_HTML);

        assert!(
            urls.iter().any(|u| u.contains("/link/abc123?sub=1")),
            "expected /link/ subscription URL, got: {urls:?}"
        );
        assert!(
            urls.iter()
                .any(|u| u.contains("subscribe?token=ABCDEF1234567890")),
            "expected /api/v1/client/subscribe token URL, got: {urls:?}"
        );
        assert!(
            urls.iter()
                .any(|u| u.contains("/sub/0123456789abcdef0123456789abcdef")),
            "expected /sub/ 32-char hash URL, got: {urls:?}"
        );
        assert!(
            urls.iter().any(|u| u.starts_with("vmess://")),
            "expected a vmess:// protocol link, got: {urls:?}"
        );
        assert!(
            urls.iter()
                .any(|u| u.contains("managed.example.com/clash.yaml")),
            "expected #MANAGED-CONFIG URL, got: {urls:?}"
        );
    }

    #[test]
    fn test_pagination_detection() {
        let html = r#"<a class="tgme_widget_message_more" href="/s/mychannel?before=12345">Load more</a>"#;
        assert_eq!(find_next_before(html), Some("12345".to_string()));

        // No pagination link -> None.
        let no_pager = r#"<a href="/s/mychannel/67890">message</a>"#;
        assert_eq!(find_next_before(no_pager), None);
    }

    #[test]
    fn test_protocol_link_detection() {
        assert!(is_protocol_link("vmess://abcdefghij123456"));
        assert!(is_protocol_link("trojan://abcdefghij"));
        // Too short after the scheme.
        assert!(!is_protocol_link("vmess://short"));
        // Not a protocol link.
        assert!(!is_protocol_link("https://example.com/sub"));
    }

    #[test]
    fn test_looks_like_subscription() {
        assert!(looks_like_subscription(
            "https://sub.example.com/link/abc123?sub=1"
        ));
        assert!(looks_like_subscription(
            "https://panel.example.com/api/v1/client/subscribe?token=ABCDEF1234567890"
        ));
        assert!(looks_like_subscription(
            "https://x.example.com/sub/0123456789abcdef0123456789abcdef"
        ));
        // Token too short to be valid (only 5 chars).
        assert!(!looks_like_subscription(
            "https://panel.example.com/api/v1/client/subscribe?token=ABCDE"
        ));
        // Not a subscription shape.
        assert!(!looks_like_subscription("https://example.com/notasub"));
        // Protocol links are not http(s) subscription URLs.
        assert!(!looks_like_subscription("vmess://abcdefghij"));
    }

    #[test]
    fn test_include_exclude_filter() {
        let url = "https://x.example.com/link/abc?sub=1";

        // Empty filters pass everything.
        assert!(passes_filter(url, "", ""));

        // Include: must contain at least one include substring.
        assert!(passes_filter(url, "link", ""));
        assert!(!passes_filter(url, "github", ""));
        // Comma-separated include: matches on the second substring.
        assert!(passes_filter(url, "github,link", ""));

        // Exclude: must not contain any exclude substring.
        assert!(!passes_filter(url, "", "link"));
        assert!(passes_filter(url, "", "github"));
        // Comma-separated exclude: excluded on the first substring.
        assert!(!passes_filter(url, "", "link,sub"));

        // Case-insensitive.
        assert!(!passes_filter(url, "", "LINK"));
        assert!(passes_filter(url, "LINK", ""));
    }

    #[test]
    fn test_extract_dedups_within_page() {
        let html = r#"
            <div class="tgme_widget_message">
              <div class="tgme_widget_message_text">https://dup.example.com/link/abc?sub=1</div>
            </div>
            <a href="https://dup.example.com/link/abc?sub=1">same</a>
        "#;
        let urls = extract_urls_from_html(html);
        assert_eq!(urls.len(), 1);
    }
}
