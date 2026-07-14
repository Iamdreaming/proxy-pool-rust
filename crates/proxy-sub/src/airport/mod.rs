//! Airport (VPN panel) auto-discovery and free-account registration.
//!
//! This module groups the pieces needed to discover airport panel sites,
//! register free accounts on them via throwaway email, and extract the
//! resulting subscription URLs:
//!
//! - [`email`] — Cloudflare Worker temp-email client.
//! - [`panel`] — panel-type probing and registerability checks.
//! - [`register`] — the [`register::AirportRegistrar`] that performs registration.
//!
//! The free functions [`discover_airport_domains`], [`load_airport_accounts`],
//! and [`save_airport_account`] tie the module into the Redis-backed store and
//! the aggregator-site crawling flow.

pub mod email;
pub mod panel;
pub mod register;

pub use email::CloudflareEmailClient;
pub use panel::{PanelType, RegisterRequirement};
pub use register::{AirportAccount, AirportRegistrar};

use chrono::{DateTime, Utc};
use proxy_core::config::AggregatorSiteConfig;
use proxy_core::store::ProxyStore;
use redis::AsyncCommands;
use std::collections::{HashMap, HashSet};
use url::Url;

/// Domains that should never be treated as airport panels.
///
/// These are common non-airport hosts (social, code hosting, search engines,
/// CDNs) that frequently appear in aggregator link lists but are not VPN
/// panels themselves.
const BLOCKLIST: &[&str] = &[
    "github.com",
    "t.me",
    "telegram",
    "google.com",
    "twitter.com",
    "x.com",
    "youtube.com",
    "facebook.com",
    "instagram.com",
    "reddit.com",
    "discord.com",
    "gstatic.com",
    "w3.org",
    "schema.org",
    "cloudflare.com",
    "microsoft.com",
    "apple.com",
    "amazon.com",
    "wikipedia.org",
    "yahoo.com",
    "bing.com",
    "baidu.com",
];

/// Discover candidate airport domains from a set of aggregator sites.
///
/// For each site the configured `format` (`html`, `json`, or `text`) selects
/// the extraction strategy. Network or parse failures for a single site are
/// logged and skipped; they never abort the whole crawl. Returned domains are
/// deduplicated, blocklist-filtered, and required to end in a plausible TLD.
pub async fn discover_airport_domains(
    sites: &[AggregatorSiteConfig],
    client: &reqwest::Client,
) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    for site in sites {
        let Some(body) = fetch_text(client, &site.url).await else {
            tracing::warn!(url = %site.url, "airport aggregator site fetch failed");
            continue;
        };
        match site.format.as_str() {
            "html" => candidates.extend(extract_from_html(&body)),
            "json" => candidates.extend(extract_from_json(&body)),
            _ => candidates.extend(extract_from_text(&body)),
        }
    }

    // Dedupe and filter to plausible airport domains.
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in candidates {
        if let Some(clean) = clean_domain(&raw)
            && seen.insert(clean.clone()) {
            out.push(clean);
        }
    }
    out
}

/// Load persisted airport accounts from the store.
///
/// Reads the `airport:accounts` set, then for each member loads the
/// `airport:accounts:{domain}` hash. Entries that fail to load are skipped.
/// Accounts missing an email are skipped (they were never successfully
/// registered).
pub async fn load_airport_accounts(store: &ProxyStore) -> Vec<AirportAccount> {
    let mut conn = store.raw_conn();
    let members: Vec<String> = match conn.smembers("airport:accounts").await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("failed to load airport account set: {e}");
            return Vec::new();
        }
    };

    let mut accounts = Vec::new();
    for domain in &members {
        let key = format!("airport:accounts:{domain}");
        let map: HashMap<String, String> = match conn.hgetall(&key).await {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(domain = %domain, "failed to load airport account hash: {e}");
                continue;
            }
        };

        let email = map.get("email").cloned().unwrap_or_default();
        if email.is_empty() {
            continue;
        }
        let password = map.get("password").cloned().unwrap_or_default();
        let token = map.get("token").cloned().filter(|t| !t.is_empty());
        let sub_url = map.get("sub_url").cloned().filter(|t| !t.is_empty());
        let panel_type = map
            .get("panel_type")
            .and_then(|v| serde_json::from_str::<PanelType>(v).ok())
            .unwrap_or(PanelType::Unknown);
        let registered_at = map
            .get("registered_at")
            .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        accounts.push(AirportAccount {
            domain: domain.clone(),
            email,
            password,
            token,
            sub_url,
            panel_type,
            registered_at,
        });
    }
    accounts
}

/// Persist an airport account to the store.
///
/// Writes the `airport:accounts` set membership and the
/// `airport:accounts:{domain}` hash with the account fields.
pub async fn save_airport_account(store: &ProxyStore, acct: &AirportAccount) -> anyhow::Result<()> {
    let mut conn = store.raw_conn();
    let key = format!("airport:accounts:{}", acct.domain);
    let _: () = conn.sadd("airport:accounts", &acct.domain).await?;
    let _: () = conn.hset(&key, "email", &acct.email).await?;
    let _: () = conn.hset(&key, "password", &acct.password).await?;
    let _: () = conn
        .hset(&key, "token", acct.token.clone().unwrap_or_default())
        .await?;
    let _: () = conn
        .hset(&key, "sub_url", acct.sub_url.clone().unwrap_or_default())
        .await?;
    let panel_type = serde_json::to_value(&acct.panel_type)?;
    let panel_type_str = panel_type.as_str().unwrap_or("unknown").to_string();
    let _: () = conn.hset(&key, "panel_type", panel_type_str).await?;
    let _: () = conn.hset(&key, "registered_at", acct.registered_at.to_rfc3339())
        .await?;
    Ok(())
}

/// Fetch a URL body as text, returning `None` on any failure (logged).
async fn fetch_text(client: &reqwest::Client, url: &str) -> Option<String> {
    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(url = %url, "airport aggregator fetch failed: {e}");
            return None;
        }
    };
    if !resp.status().is_success() {
        tracing::warn!(
            url = %url,
            status = %resp.status(),
            "airport aggregator unexpected status"
        );
        return None;
    }
    match resp.text().await {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!(url = %url, "airport aggregator read body failed: {e}");
            None
        }
    }
}

/// Extract candidate domain strings from an HTML aggregator page.
fn extract_from_html(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let doc = scraper::Html::parse_document(body);
    if let Ok(sel) = scraper::Selector::parse("a") {
        for a in doc.select(&sel) {
            if let Some(href) = a.value().attr("href") {
                out.extend(extract_hosts(href));
            }
            let text = a.text().collect::<Vec<_>>().join(" ");
            out.extend(extract_hosts(&text));
        }
    }
    // Also scan the plain document text line-by-line.
    let doc_text = doc.root_element().text().collect::<Vec<_>>().join("\n");
    for line in doc_text.lines() {
        out.extend(extract_hosts(line));
    }
    out
}

/// Extract candidate domain strings from a JSON aggregator response.
///
/// Accepts either an array of `{ "domain": "..." }` objects or an array of
/// plain strings.
fn extract_from_json(body: &str) -> Vec<String> {
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("airport json parse failed: {e}");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    if let Some(arr) = v.as_array() {
        for item in arr {
            if let Some(obj) = item.as_object() {
                if let Some(d) = obj.get("domain").and_then(|x| x.as_str()) {
                    out.push(d.to_string());
                }
            } else if let Some(s) = item.as_str() {
                out.push(s.to_string());
            }
        }
    }
    out
}

/// Extract candidate domain strings from a plain-text aggregator response,
/// one domain per line. Blank lines and `#` comments are skipped.
fn extract_from_text(body: &str) -> Vec<String> {
    body.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect()
}

/// Extract candidate host strings from a single text/href blob.
fn extract_hosts(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    // A full URL: take its host.
    if let Ok(url) = Url::parse(text.trim())
        && let Some(host) = url.host_str() {
        out.push(host.to_string());
        return out;
    }
    // Otherwise scan whitespace-delimited tokens for domain-like strings.
    for token in text.split_whitespace() {
        let token = token.trim_matches(|c: char| c.is_ascii_punctuation() && c != '.');
        if token.contains('.') {
            out.push(token.to_string());
        }
    }
    out
}

/// Normalize and validate a candidate domain string.
///
/// Returns `None` if the string has no dot, matches a blocklisted host/label,
/// or does not end in a plausible TLD (an ASCII alphabetic label 2–6 chars
/// long).
fn clean_domain(raw: &str) -> Option<String> {
    let raw = raw.trim().to_lowercase();
    if !raw.contains('.') {
        return None;
    }
    for blocked in BLOCKLIST {
        if raw.contains(blocked) {
            return None;
        }
    }
    let host = match Url::parse(&raw) {
        Ok(url) => url.host_str()?.to_string(),
        Err(_) => raw,
    };
    let host = host.trim_end_matches('.');
    let last_label = host.rsplit('.').next()?;
    if !(2..=6).contains(&last_label.len()) || !last_label.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    Some(host.to_string())
}
