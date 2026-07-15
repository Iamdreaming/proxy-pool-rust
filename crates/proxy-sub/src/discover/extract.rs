//! Shared free-text subscription/protocol URL extraction.
//!
//! This module is the single source of truth for the heuristic URL extraction
//! logic used by discoverers that scrape free-form text (Telegram message
//! bodies, LLM web-search result blobs, etc.). It classifies tokens as either
//! proxy subscription URLs or direct protocol links using string methods only
//! — no regex dependency is used.

use std::collections::HashSet;

/// Extract all subscription/protocol URLs from a free-text blob, deduplicated.
pub(crate) fn extract_subscription_urls(blob: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    extract_from_blob(blob, &mut seen, &mut out);
    out
}

/// Classify and extract URLs from a single text/href blob.
pub(crate) fn extract_from_blob(blob: &str, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    // `#MANAGED-CONFIG` / `#订阅链接` comment lines carry a plain URL on the
    // same line; those URLs are not otherwise recognized as subscriptions.
    if blob.contains("#MANAGED-CONFIG") || blob.contains("#订阅链接") {
        for line in blob.split('\n') {
            if line.contains("#MANAGED-CONFIG") || line.contains("#订阅链接") {
                for token in tokenize(line) {
                    let trimmed = trim_trailing_punct(token);
                    if is_http_url(trimmed) {
                        push_unique(trimmed.to_string(), seen, out);
                    }
                }
            }
        }
    }

    for token in tokenize(blob) {
        let trimmed = trim_trailing_punct(token);
        if looks_like_subscription(trimmed)
            || looks_like_sub_file(trimmed)
            || is_protocol_link(trimmed)
        {
            push_unique(trimmed.to_string(), seen, out);
        }
    }
}

/// Split a blob into candidate tokens on whitespace and link-breaking punctuation.
pub(crate) fn tokenize(text: &str) -> Vec<&str> {
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
pub(crate) fn is_http_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Heuristically detect whether a token is a proxy subscription URL.
///
/// Matches three common shapes without regex:
/// - `/api/v1/client/subscribe?token=` with a 16–32 char alphanumeric token
/// - `/link/{id}?sub=` / `?mu=` / `?clash=`
/// - `/sub/{16-64 char alphanumeric hash}`
pub(crate) fn looks_like_subscription(url: &str) -> bool {
    if !is_http_url(url) {
        return false;
    }

    if let Some(idx) = url.find("subscribe?token=") {
        let token = &url[idx + "subscribe?token=".len()..];
        let token_val = token
            .split(|c: char| !c.is_ascii_alphanumeric())
            .next()
            .unwrap_or("");
        return (16..=32).contains(&token_val.len());
    }

    if url.contains("/link/") {
        return url.contains("?sub=") || url.contains("?mu=") || url.contains("?clash=");
    }

    if let Some(idx) = url.find("/sub/") {
        let rest = &url[idx + "/sub/".len()..];
        let hash = rest
            .split(|c: char| !c.is_ascii_alphanumeric())
            .next()
            .unwrap_or("");
        return (16..=64).contains(&hash.len());
    }

    false
}

/// Whether `token` is a direct protocol link (>= 10 chars after the scheme).
pub(crate) fn is_protocol_link(token: &str) -> bool {
    for prefix in crate::models::PROTOCOL_LINK_SCHEMES {
        if let Some(stripped) = token.strip_prefix(prefix) {
            return stripped.len() >= 10;
        }
    }
    false
}

/// Whether `url` looks like a raw subscription *content* file commonly surfaced
/// by web search (clash/v2ray config files, `/sub` endpoints), as opposed to a
/// panel API link. Matches on well-known content suffixes and `/sub` endpoints.
pub(crate) fn looks_like_sub_file(url: &str) -> bool {
    if !is_http_url(url) {
        return false;
    }
    // Strip query/fragment before inspecting the path suffix.
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with(".txt")
        || lower.ends_with("/sub")
}

/// Strip trailing sentence/CJK punctuation that prose commonly appends to an
/// inline URL (e.g. a period ending the sentence). Never strips `/`.
pub(crate) fn trim_trailing_punct(token: &str) -> &str {
    token.trim_end_matches(|c: char| {
        matches!(
            c,
            '.' | ','
                | ';'
                | ':'
                | '!'
                | '?'
                | '。'
                | '、'
                | '，'
                | '；'
                | '：'
                | '？'
                | '！'
                | '）'
                | '】'
                | '》'
                | '”'
        )
    })
}

/// Append `url` to `out` unless it has already been seen.
pub(crate) fn push_unique(url: String, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    if seen.insert(url.clone()) {
        out.push(url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_extract_dedups_within_blob() {
        let blob = "https://dup.example.com/link/abc?sub=1 \
                    and again https://dup.example.com/link/abc?sub=1";
        let urls = extract_subscription_urls(blob);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://dup.example.com/link/abc?sub=1");
    }

    #[test]
    fn test_extract_managed_config_line() {
        let blob = "#MANAGED-CONFIG https://managed.example.com/clash.yaml";
        let urls = extract_subscription_urls(blob);
        assert!(
            urls.iter()
                .any(|u| u.contains("managed.example.com/clash.yaml")),
            "expected #MANAGED-CONFIG URL, got: {urls:?}"
        );
    }

    #[test]
    fn test_looks_like_sub_file() {
        assert!(looks_like_sub_file(
            "https://raw.githubusercontent.com/x/y/main/clash.yaml"
        ));
        assert!(looks_like_sub_file(
            "https://static.v2rayshare.net/2026/07/20260714.yaml"
        ));
        assert!(looks_like_sub_file(
            "https://raw.githubusercontent.com/Pawdroid/Free-servers/main/sub"
        ));
        assert!(looks_like_sub_file("https://x.com/free.txt"));
        // Query string after a content suffix still matches.
        assert!(looks_like_sub_file("https://x.com/clash.yaml?token=1"));
        // Not content files.
        assert!(!looks_like_sub_file("https://github.com/user/repo"));
        assert!(!looks_like_sub_file("https://awesome-vpn.github.io/"));
        // Non-http rejected.
        assert!(!looks_like_sub_file("vmess://abcdefghij"));
    }

    #[test]
    fn test_trim_trailing_punct() {
        assert_eq!(
            trim_trailing_punct("https://example.com/link"),
            "https://example.com/link"
        );
        assert_eq!(
            trim_trailing_punct("https://a.com/x.yaml."),
            "https://a.com/x.yaml"
        );
        assert_eq!(
            trim_trailing_punct("https://example.com/sub。"),
            "https://example.com/sub"
        );
        assert_eq!(
            trim_trailing_punct("https://example.com/link.。？"),
            "https://example.com/link"
        );
        // Forward slash and interior dots are preserved.
        assert_eq!(
            trim_trailing_punct("https://example.com/"),
            "https://example.com/"
        );
        assert_eq!(
            trim_trailing_punct("https://raw.githubusercontent.com/a/clash.yaml"),
            "https://raw.githubusercontent.com/a/clash.yaml"
        );
    }

    #[test]
    fn test_extract_grok_raw_urls_with_trailing_period() {
        // Prose with a raw clash.yaml URL followed by a sentence period, and a
        // panel URL also followed by a period — both must be extracted CLEAN.
        let blob = "See https://raw.githubusercontent.com/x/y/main/clash.yaml. \
                    Panel: https://foo.com/api/v1/client/subscribe?token=abcdef1234567890.";
        let urls = extract_subscription_urls(blob);
        assert!(
            urls.contains(&"https://raw.githubusercontent.com/x/y/main/clash.yaml".to_string()),
            "raw yaml URL should be extracted clean, got: {urls:?}"
        );
        assert!(
            urls.contains(
                &"https://foo.com/api/v1/client/subscribe?token=abcdef1234567890".to_string()
            ),
            "panel URL should be extracted without trailing period, got: {urls:?}"
        );
    }
}
