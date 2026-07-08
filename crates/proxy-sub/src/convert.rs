//! Proxy conversion: `SubscriptionProxy` → `Proxy`, and partitioning.

use crate::models::SubscriptionProxy;
use proxy_core::models::Proxy;

/// Convert a `SubscriptionProxy` into a pool-usable `Proxy`.
///
/// Only `SubscriptionProxy::Basic` variants are convertible — they represent
/// plain socks5/http/https proxies. Encrypted variants (SS, VMess, Trojan,
/// VLESS)
/// return `None` because they require local relay setup before they can join
/// the pool.
///
/// The `source_url` is recorded as `subscription:{source_url}` on the
/// resulting `Proxy`.
pub fn to_proxy(sub: &SubscriptionProxy, source_url: &str) -> Option<Proxy> {
    match sub {
        SubscriptionProxy::Basic {
            host,
            port,
            protocol,
        } => {
            let mut proxy = Proxy::new(host.clone(), *port, *protocol);
            proxy.source = Some(format!("subscription:{source_url}"));
            Some(proxy)
        }
        _ => None,
    }
}

/// Partition a slice of `SubscriptionProxy` into:
/// - `(Vec<Proxy>, …)` — basic proxies converted for the pool.
/// - `(…, Vec<SubscriptionProxy>)` — encrypted proxies that need further setup.
///
/// Unknown/unsupported entries are skipped here and only counted by callers.
pub fn partition(
    subs: &[SubscriptionProxy],
    source_url: &str,
) -> (Vec<Proxy>, Vec<SubscriptionProxy>) {
    let mut basics = Vec::new();
    let mut encrypted = Vec::new();

    for sub in subs {
        if let Some(proxy) = to_proxy(sub, source_url) {
            basics.push(proxy);
        } else if matches!(
            sub,
            SubscriptionProxy::Shadowsocks { .. }
                | SubscriptionProxy::Vmess { .. }
                | SubscriptionProxy::Trojan { .. }
                | SubscriptionProxy::Vless { .. }
        ) {
            encrypted.push(sub.clone());
        }
    }

    (basics, encrypted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxy_core::models::Protocol;

    #[test]
    fn test_to_proxy_basic() {
        let sub = SubscriptionProxy::Basic {
            host: "1.2.3.4".into(),
            port: 1080,
            protocol: Protocol::Socks5,
        };
        let proxy = to_proxy(&sub, "https://example.com/sub").unwrap();
        assert_eq!(proxy.host, "1.2.3.4");
        assert_eq!(proxy.port, 1080);
        assert_eq!(proxy.protocol, Protocol::Socks5);
        assert_eq!(
            proxy.source.as_deref(),
            Some("subscription:https://example.com/sub")
        );
    }

    #[test]
    fn test_to_proxy_encrypted_returns_none() {
        let ss = SubscriptionProxy::Shadowsocks {
            host: "5.6.7.8".into(),
            port: 8388,
            method: "aes-256-gcm".into(),
            password: "pass".into(),
            plugin: None,
            plugin_opts: None,
        };
        assert!(to_proxy(&ss, "https://example.com/sub").is_none());

        let vmess = SubscriptionProxy::Vmess {
            host: "9.10.11.12".into(),
            port: 443,
            uuid: "uid".into(),
            alter_id: 0,
            security: "auto".into(),
            network: "ws".into(),
            path: None,
            host_header: None,
            sni: None,
        };
        assert!(to_proxy(&vmess, "https://example.com/sub").is_none());
    }

    #[test]
    fn test_partition() {
        let subs = vec![
            SubscriptionProxy::Basic {
                host: "1.1.1.1".into(),
                port: 1080,
                protocol: Protocol::Socks5,
            },
            SubscriptionProxy::Shadowsocks {
                host: "2.2.2.2".into(),
                port: 8388,
                method: "aes-256-gcm".into(),
                password: "pass".into(),
                plugin: None,
                plugin_opts: None,
            },
            SubscriptionProxy::Basic {
                host: "3.3.3.3".into(),
                port: 8080,
                protocol: Protocol::Http,
            },
            SubscriptionProxy::Trojan {
                host: "4.4.4.4".into(),
                port: 443,
                password: "pw".into(),
                sni: None,
                network: None,
            },
            SubscriptionProxy::Unknown {
                raw_config: "vless://unsupported".into(),
            },
            SubscriptionProxy::Vless {
                host: "5.5.5.5".into(),
                port: 443,
                uuid: "uid".into(),
                encryption: "none".into(),
                flow: None,
                network: "tcp".into(),
                security: None,
                sni: None,
                host_header: None,
                path: None,
                service_name: None,
                fingerprint: None,
                public_key: None,
                short_id: None,
                spider_x: None,
            },
        ];

        let (basics, encrypted) = partition(&subs, "https://sub.example.com");

        assert_eq!(basics.len(), 2);
        assert_eq!(basics[0].host, "1.1.1.1");
        assert_eq!(basics[1].host, "3.3.3.3");
        assert_eq!(
            basics[0].source.as_deref(),
            Some("subscription:https://sub.example.com")
        );

        assert_eq!(encrypted.len(), 3);
        assert!(matches!(
            encrypted[0],
            SubscriptionProxy::Shadowsocks { .. }
        ));
        assert!(matches!(encrypted[1], SubscriptionProxy::Trojan { .. }));
        assert!(matches!(encrypted[2], SubscriptionProxy::Vless { .. }));
    }
}
