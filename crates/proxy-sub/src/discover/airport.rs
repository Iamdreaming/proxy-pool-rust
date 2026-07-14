//! Airport auto-discovery discoverer.
//!
//! [`AirportDiscover`] discovers airport (VPN panel) sites from configured
//! aggregator pages, probes and registers free accounts on registerable ones,
//! and returns the resulting subscription URLs. Failures for any single site
//! are logged and never abort the overall flow; partial results are returned.

use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use proxy_core::store::ProxyStore;

use crate::airport::{
    discover_airport_domains, load_airport_accounts, save_airport_account, AirportAccount,
    AirportRegistrar,
};
use crate::airport::panel::{is_registerable, probe_panel};
use crate::discover::Discover;

/// Configuration for [`AirportDiscover`].
#[derive(Debug, Clone)]
pub struct AirportConfig {
    /// Aggregator sites that list candidate airport domains.
    pub aggregator_sites: Vec<proxy_core::config::AggregatorSiteConfig>,
    /// Base URL of the Cloudflare Worker temp-email service.
    pub cloudflare_worker_url: String,
    /// Optional admin auth token for the temp-email worker.
    pub cloudflare_admin_auth: Option<String>,
    /// Maximum number of airport registrations to run concurrently.
    pub max_concurrent: usize,
    /// Per-request HTTP timeout, in seconds.
    pub timeout_sec: u64,
}

/// Auto-discovers airport sites, registers free accounts, returns subscription URLs.
pub struct AirportDiscover {
    config: AirportConfig,
    client: reqwest::Client,
    store: Option<Arc<ProxyStore>>,
}

impl AirportDiscover {
    /// Create a new airport discoverer with the given configuration.
    pub fn new(config: AirportConfig, store: Option<Arc<ProxyStore>>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_sec))
            .user_agent("proxy-pool-rust")
            .build()
            .unwrap_or_default();
        Self {
            config,
            client,
            store,
        }
    }
}

#[async_trait::async_trait]
impl Discover for AirportDiscover {
    fn name(&self) -> &str {
        "airport"
    }

    async fn discover(&self) -> Vec<String> {
        let mut result: Vec<String> = Vec::new();

        // Step 1-2: surface subscription URLs from already-registered accounts.
        if let Some(store) = &self.store {
            let accounts = load_airport_accounts(store).await;
            for a in &accounts {
                if let Some(u) = &a.sub_url {
                    result.push(u.clone());
                }
            }
        }

        // Step 3: discover candidate domains from aggregator sites.
        let domains = discover_airport_domains(&self.config.aggregator_sites, &self.client).await;

        // Step 4: skip domains we already have accounts for.
        let known: std::collections::HashSet<String> = if let Some(store) = &self.store {
            load_airport_accounts(store)
                .await
                .into_iter()
                .map(|a| a.domain)
                .collect()
        } else {
            std::collections::HashSet::new()
        };
        let new_domains: Vec<String> = domains
            .into_iter()
            .filter(|d| !known.contains(d))
            .collect();
        if new_domains.is_empty() {
            return dedupe(result);
        }

        // Register on each new domain concurrently (bounded).
        let max_concurrent = self.config.max_concurrent.max(1);
        let entries: Vec<Option<AirportAccount>> = stream::iter(new_domains)
            .map(|domain: String| {
                let client = self.client.clone();
                let registrar = AirportRegistrar::new(
                    self.config.cloudflare_worker_url.clone(),
                    self.config.cloudflare_admin_auth.clone(),
                );
                let store = self.store.clone();
                async move {
                    let req = match probe_panel(&domain, &client).await {
                        Some(r) => r,
                        None => {
                            tracing::info!(domain = %domain, "airport probe failed, skipping");
                            return None;
                        }
                    };
                    if !is_registerable(&req) {
                        tracing::info!(domain = %domain, "airport not registerable, skipping");
                        return None;
                    }
                    match registrar.register_airport(&domain, &req).await {
                        Ok(acct) => {
                            if let Some(s) = &store
                                && let Err(e) = save_airport_account(s, &acct).await {
                                tracing::warn!(
                                    domain = %domain,
                                    "failed to persist airport account: {e}"
                                );
                            }
                            Some(acct)
                        }
                        Err(e) => {
                            tracing::warn!(domain = %domain, "airport registration failed: {e}");
                            None
                        }
                    }
                }
            })
            .buffer_unordered(max_concurrent)
            .collect()
            .await;

        for acct in entries.into_iter().flatten() {
            if let Some(u) = &acct.sub_url {
                result.push(u.clone());
            }
        }

        dedupe(result)
    }
}

/// Deduplicate a list of URLs while preserving first-seen order.
fn dedupe(mut urls: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    urls.retain(|u| seen.insert(u.clone()));
    urls
}
