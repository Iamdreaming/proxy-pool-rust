//! Deduplication of proxy lists.

use crate::models::Proxy;

/// Remove duplicates by (protocol, host, port). Keeps first occurrence.
pub fn dedup(proxies: Vec<Proxy>) -> Vec<Proxy> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::with_capacity(proxies.len());
    for p in proxies {
        let key = p.dedup_key();
        if seen.insert(key) {
            result.push(p);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Protocol;

    #[test]
    fn test_dedup() {
        let proxies = vec![
            Proxy::new("1.1.1.1", 80, Protocol::Http),
            Proxy::new("1.1.1.1", 80, Protocol::Http), // dup
            Proxy::new("1.1.1.1", 8080, Protocol::Http), // different port
            Proxy::new("1.1.1.1", 80, Protocol::Socks5), // different protocol
        ];
        let result = dedup(proxies);
        assert_eq!(result.len(), 3);
    }
}
