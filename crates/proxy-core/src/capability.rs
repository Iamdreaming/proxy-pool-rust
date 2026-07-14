//! Node capability tagging.
//!
//! Probes proxies against well-known service endpoints (e.g. ChatGPT, OpenAI)
//! and records which capabilities a given proxy node satisfies. Tags are stored
//! in Redis so the gateway can prefer capable proxies for matching hosts.
//!
//! Storage layout:
//! - `proxy:capabilities:{proxy_key}` — a set of JSON-serialized `CapabilityTag`.
//! - `proxy:capability_index:{tag}` — a set of `proxy_key` values (reverse index).

use std::fmt;
use std::str::FromStr;

use anyhow::Result;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use crate::models::Proxy;
use crate::validator::build_reqwest_proxy;

/// A capability a proxy node may satisfy.
///
/// The snake_case serde names are the stable tag identifiers stored in Redis
/// and referenced from configuration (`capabilities.targets[].tag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityTag {
    /// Access to chat.openai.com (and generally OpenAI web surfaces).
    #[serde(rename = "chat_gpt")]
    ChatGPT,
    /// Access to the OpenAI API (`api.openai.com`).
    #[serde(rename = "openai")]
    OpenAI,
    /// Access to YouTube.
    #[serde(rename = "youtube")]
    YouTube,
    /// Access to Google services.
    #[serde(rename = "google")]
    Google,
}

impl CapabilityTag {
    /// Stable Redis/index string for this tag.
    pub fn as_str(&self) -> &'static str {
        match self {
            CapabilityTag::ChatGPT => "chat_gpt",
            CapabilityTag::OpenAI => "openai",
            CapabilityTag::YouTube => "youtube",
            CapabilityTag::Google => "google",
        }
    }

    /// All defined capability tags.
    pub fn all() -> &'static [CapabilityTag] {
        &[
            CapabilityTag::ChatGPT,
            CapabilityTag::OpenAI,
            CapabilityTag::YouTube,
            CapabilityTag::Google,
        ]
    }
}

impl fmt::Display for CapabilityTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CapabilityTag {
    type Err = anyhow::Error;

    /// Parse a tag from its snake_case string (e.g. `"chat_gpt"` -> `ChatGPT`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(&format!("\"{s}\"")).map_err(Into::into)
    }
}

/// A capability target the scheduler probes a candidate proxy against.
#[derive(Debug, Clone)]
pub struct CapabilityTarget {
    /// Human-readable name used for logging.
    pub name: String,
    /// URL to request through the candidate proxy.
    pub url: String,
    /// HTTP status expected when the proxy satisfies the capability.
    pub expected_status: u16,
    /// Tag assigned on a successful probe.
    pub tag: CapabilityTag,
}

/// Redis-backed store for proxy capability tags.
#[derive(Clone)]
pub struct CapabilityStore {
    conn: redis::aio::MultiplexedConnection,
}

impl CapabilityStore {
    /// Build a capability store from an existing Redis multiplexed connection.
    pub fn new(conn: redis::aio::MultiplexedConnection) -> Self {
        Self { conn }
    }

    /// Tag `proxy_key` with `tag`, updating both the forward and reverse indexes.
    pub async fn assign(&self, proxy_key: &str, tag: &CapabilityTag) -> Result<()> {
        let mut conn = self.conn.clone();
        let tag_json = serde_json::to_string(tag)?;
        let cap_key = format!("proxy:capabilities:{proxy_key}");
        let idx_key = format!("proxy:capability_index:{}", tag.as_str());
        let _: () = conn.sadd(cap_key, tag_json).await?;
        let _: () = conn.sadd(idx_key, proxy_key).await?;
        Ok(())
    }

    /// Remove `tag` from `proxy_key`, cleaning up both indexes.
    pub async fn remove(&self, proxy_key: &str, tag: &CapabilityTag) -> Result<()> {
        let mut conn = self.conn.clone();
        let tag_json = serde_json::to_string(tag)?;
        let cap_key = format!("proxy:capabilities:{proxy_key}");
        let idx_key = format!("proxy:capability_index:{}", tag.as_str());
        let _: () = conn.srem(cap_key, tag_json).await?;
        let _: () = conn.srem(idx_key, proxy_key).await?;
        Ok(())
    }

    /// Return all tags currently assigned to `proxy_key`.
    pub async fn get(&self, proxy_key: &str) -> Result<Vec<CapabilityTag>> {
        let mut conn = self.conn.clone();
        let cap_key = format!("proxy:capabilities:{proxy_key}");
        let members: Vec<String> = conn.smembers(cap_key).await?;
        let mut tags = Vec::new();
        for m in members {
            match serde_json::from_str::<CapabilityTag>(&m) {
                Ok(t) => tags.push(t),
                Err(e) => tracing::warn!(
                    key = %proxy_key,
                    value = %m,
                    "capability get: dropping unparseable tag: {e}"
                ),
            }
        }
        Ok(tags)
    }

    /// Return all proxy keys tagged with `tag` (reverse index lookup).
    pub async fn get_proxies_with_tag(&self, tag: &CapabilityTag) -> Result<Vec<String>> {
        let mut conn = self.conn.clone();
        let idx_key = format!("proxy:capability_index:{}", tag.as_str());
        let members: Vec<String> = conn.smembers(idx_key).await?;
        Ok(members)
    }

    /// Probe `proxy` against `target` and report whether it satisfies the
    /// capability. A failed probe (network error or timeout) yields `Ok(false)`
    /// — it never removes an existing tag.
    pub async fn test_capability(&self, proxy: &Proxy, target: &CapabilityTarget) -> Result<bool> {
        let client = reqwest::Client::builder()
            .proxy(build_reqwest_proxy(proxy)?)
            .timeout(std::time::Duration::from_secs(5))
            .connect_timeout(std::time::Duration::from_secs(5))
            .pool_max_idle_per_host(0)
            .build()?;

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.get(&target.url).send(),
        )
        .await;

        match result {
            Ok(Ok(resp)) => Ok(resp.status().as_u16() == target.expected_status),
            Ok(Err(e)) => {
                tracing::debug!(
                    proxy = %proxy.key(),
                    target = %target.name,
                    "capability test request failed: {e}"
                );
                Ok(false)
            }
            Err(_) => {
                tracing::debug!(
                    proxy = %proxy.key(),
                    target = %target.name,
                    "capability test timed out"
                );
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_as_str_and_display_match() {
        assert_eq!(CapabilityTag::ChatGPT.as_str(), "chat_gpt");
        assert_eq!(CapabilityTag::OpenAI.as_str(), "openai");
        assert_eq!(CapabilityTag::YouTube.as_str(), "youtube");
        assert_eq!(CapabilityTag::Google.as_str(), "google");
        assert_eq!(CapabilityTag::ChatGPT.to_string(), "chat_gpt");
    }

    #[test]
    fn tag_from_str_round_trips() {
        assert_eq!(
            CapabilityTag::from_str("chat_gpt").unwrap(),
            CapabilityTag::ChatGPT
        );
        assert_eq!(
            CapabilityTag::from_str("openai").unwrap(),
            CapabilityTag::OpenAI
        );
        assert_eq!(
            CapabilityTag::from_str("youtube").unwrap(),
            CapabilityTag::YouTube
        );
        assert_eq!(
            CapabilityTag::from_str("google").unwrap(),
            CapabilityTag::Google
        );
        assert!(CapabilityTag::from_str("nope").is_err());
    }

    #[test]
    fn tag_serde_round_trips() {
        for tag in CapabilityTag::all() {
            let json = serde_json::to_string(tag).unwrap();
            assert_eq!(json, format!("\"{}\"", tag.as_str()));
            let back: CapabilityTag = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, tag);
        }
    }

    #[test]
    fn all_tags_present() {
        assert_eq!(CapabilityTag::all().len(), 4);
    }
}
