//! Parser trait and format-auto-detection entrypoint.

use crate::models::SubscriptionProxy;

mod base64_uri;
mod clash;
mod surge;
mod v2ray_json;

pub use base64_uri::Base64UriParser;
pub use clash::ClashParser;
pub use surge::SurgeParser;
pub use v2ray_json::V2rayJsonParser;

/// A subscription format parser: detect format, parse content.
pub trait Parser: Send + Sync {
    /// Human-readable name for logging.
    fn name(&self) -> &str;

    /// Check whether raw content matches this format.
    fn detect(&self, content: &str) -> bool;

    /// Parse content into a list of subscription proxies.
    /// Malformed entries are skipped with a warning log.
    fn parse(&self, content: &str) -> Vec<SubscriptionProxy>;
}

/// All built-in parsers in detection order.
pub fn builtin_parsers() -> Vec<Box<dyn Parser>> {
    vec![
        Box::new(V2rayJsonParser), // JSON: fast reject if not valid JSON
        Box::new(ClashParser),     // YAML: check for `proxies:` key
        Box::new(Base64UriParser), // Base64: decode + check for `://`
        Box::new(SurgeParser),     // Line regex
    ]
}

/// Auto-detect format and parse content using built-in parsers.
/// First matching parser wins. Returns empty vec if no parser matches.
pub fn parse_subscription(content: &str) -> Vec<SubscriptionProxy> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    for parser in builtin_parsers() {
        if parser.detect(trimmed) {
            tracing::info!("subscription parser: detected {} format", parser.name());
            return parser.parse(trimmed);
        }
    }

    tracing::warn!(
        "subscription parser: no format detected for content (len={})",
        trimmed.len()
    );
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_subscription_empty() {
        assert!(parse_subscription("").is_empty());
        assert!(parse_subscription("   ").is_empty());
    }

    #[test]
    fn test_parse_subscription_no_match() {
        assert!(parse_subscription("hello world\nfoo bar").is_empty());
    }

    #[test]
    fn test_parse_subscription_surge() {
        let content = "socks5-proxy = socks5, 10.0.0.1, 1080\nhttp-proxy = http, 10.0.0.2, 8080";
        let proxies = parse_subscription(content);
        assert_eq!(proxies.len(), 2);

        use crate::models::SubscriptionProxy;
        use proxy_core::models::Protocol;

        if let SubscriptionProxy::Basic {
            host,
            port,
            protocol,
        } = &proxies[0]
        {
            assert_eq!(host, "10.0.0.1");
            assert_eq!(*port, 1080);
            assert_eq!(*protocol, Protocol::Socks5);
        } else {
            panic!("Expected Basic (socks5), got {:?}", proxies[0]);
        }

        if let SubscriptionProxy::Basic {
            host,
            port,
            protocol,
        } = &proxies[1]
        {
            assert_eq!(host, "10.0.0.2");
            assert_eq!(*port, 8080);
            assert_eq!(*protocol, Protocol::Http);
        } else {
            panic!("Expected Basic (http), got {:?}", proxies[1]);
        }
    }

    #[test]
    fn test_parse_subscription_no_match_fixture() {
        let content = include_str!("../../tests/fixtures/mixed_invalid.txt");
        let proxies = parse_subscription(content);
        assert!(proxies.is_empty());
    }
}
