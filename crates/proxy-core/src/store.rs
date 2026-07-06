//! Redis-backed proxy storage with ZSet scoring per protocol.

use crate::circuit::{self, CircuitBreakerConfig};
use crate::config::ScoreWeights;
use crate::models::{Anonymity, Protocol, Proxy, ProxyFilter};
use redis::AsyncCommands;
use redis::aio::MultiplexedConnection;
use std::sync::Arc;

/// Compute a 0..1 score from latency, success rate, anonymity.
pub fn score(proxy: &Proxy, weights: &ScoreWeights) -> f64 {
    let latency = proxy.latency_ms.unwrap_or(5000.0);
    // Inverse-latency normalization: 0ms→1.0, 2000ms→0.0, linear between.
    let latency_norm = ((2000.0 - latency) / 2000.0).clamp(0.0, 1.0);

    let total = proxy.success_count + proxy.fail_count;
    let success_rate = if total == 0 {
        0.5 // untested: neutral
    } else {
        ((proxy.success_count as f64 - proxy.fail_count as f64) / total as f64).clamp(0.0, 1.0)
    };

    let anonymity = proxy.anonymity.map(|a| a.bonus()).unwrap_or(0.0);

    weights.latency * latency_norm + weights.success * success_rate + weights.anonymity * anonymity
}

/// Weighted random choice: prefer higher-scored proxies.
pub fn weighted_random_choice(
    proxies: &[Proxy],
    score_fn: impl Fn(&Proxy) -> f64,
) -> Option<Proxy> {
    if proxies.is_empty() {
        return None;
    }
    let scores: Vec<f64> = proxies.iter().map(&score_fn).collect();
    let total: f64 = scores.iter().sum();
    if total <= 0.0 {
        // All zero scores: uniform random
        let idx = (rand::random::<u64>() as usize) % proxies.len();
        return Some(proxies[idx].clone());
    }
    let mut r = rand::random::<f64>() * total;
    for (i, s) in scores.iter().enumerate() {
        r -= s;
        if r <= 0.0 {
            return Some(proxies[i].clone());
        }
    }
    Some(proxies.last().unwrap().clone())
}

fn redis_key(protocol: &Protocol) -> String {
    format!("proxies:{protocol}")
}

/// Redis-backed proxy storage with ZSet scoring per protocol.
///
/// Uses `Arc<MultiplexedConnection>` because redis 0.27's `AsyncCommands`
/// requires `&mut self`. `MultiplexedConnection` is cheaply cloneable.
pub struct ProxyStore {
    conn: Arc<MultiplexedConnection>,
    weights: ScoreWeights,
    min_score: f64,
    circuit_config: CircuitBreakerConfig,
}

impl ProxyStore {
    pub fn new(
        conn: MultiplexedConnection,
        weights: ScoreWeights,
        min_score: f64,
        circuit_config: CircuitBreakerConfig,
    ) -> Self {
        Self {
            conn: Arc::new(conn),
            weights,
            min_score,
            circuit_config,
        }
    }

    fn conn(&self) -> MultiplexedConnection {
        // MultiplexedConnection implements Clone — each clone shares the
        // same underlying connection multiplexer.
        (*self.conn).clone()
    }

    /// Add a proxy to the store (upsert by dedup key).
    ///
    /// Removes any existing entry for the same logical proxy (host:port:protocol)
    /// before inserting, so that stale stats don't create duplicate ZSET members.
    pub async fn add(&self, proxy: &Proxy) -> anyhow::Result<()> {
        self.remove_existing(&proxy.protocol, proxy).await?;
        let s = score(proxy, &self.weights);
        let member = serde_json::to_string(proxy)?;
        let key = redis_key(&proxy.protocol);
        let mut conn = self.conn();
        let _: () = conn.zadd(&key, &member, s).await?;
        Ok(())
    }

    /// Get the highest-scored proxy for a protocol, excluding circuit-open proxies.
    pub async fn get_best(&self, protocol: Protocol) -> anyhow::Result<Option<Proxy>> {
        let key = redis_key(&protocol);
        let mut conn = self.conn();
        // Fetch top 10 candidates and filter circuit-open ones
        let members: Vec<String> = conn.zrevrange(&key, 0, 9).await?;
        for m in members {
            let proxy: Proxy = serde_json::from_str(&m)?;
            if !circuit::is_circuit_open(&proxy) {
                return Ok(Some(proxy));
            }
        }
        Ok(None)
    }

    /// Get a random proxy (biased toward higher scores), excluding circuit-open proxies.
    pub async fn get_random(&self, protocol: Protocol) -> anyhow::Result<Option<Proxy>> {
        let key = redis_key(&protocol);
        let mut conn = self.conn();
        let members: Vec<String> = conn.zrevrange(&key, 0, -1).await?;
        if members.is_empty() {
            return Ok(None);
        }
        // Parse and filter circuit-open proxies
        let proxies: Vec<Proxy> = members
            .iter()
            .filter_map(|m| serde_json::from_str::<Proxy>(m).ok())
            .filter(|p| !circuit::is_circuit_open(p))
            .collect();
        if proxies.is_empty() {
            return Ok(None);
        }
        let score_fn = |p: &Proxy| score(p, &self.weights);
        Ok(weighted_random_choice(&proxies, score_fn))
    }

    /// Get overseas proxies (is_overseas == true).
    pub async fn get_overseas(
        &self,
        protocol: Protocol,
        limit: usize,
    ) -> anyhow::Result<Vec<Proxy>> {
        let all = self.all(protocol).await?;
        Ok(all
            .into_iter()
            .filter(|p| p.is_overseas)
            .take(limit)
            .collect())
    }

    /// Get domestic proxies (is_overseas == false).
    pub async fn get_domestic(
        &self,
        protocol: Protocol,
        limit: usize,
    ) -> anyhow::Result<Vec<Proxy>> {
        let all = self.all(protocol).await?;
        Ok(all
            .into_iter()
            .filter(|p| !p.is_overseas)
            .take(limit)
            .collect())
    }

    /// Mark a proxy as failed; evict if below threshold.
    pub async fn mark_failed(&self, proxy: &Proxy) -> anyhow::Result<()> {
        self.remove_existing(&proxy.protocol, proxy).await?;
        let mut updated = proxy.clone();
        updated.fail_count += 1;

        // Hard eviction: too many failures
        let hard_evict = updated.fail_count > std::cmp::max(8, updated.success_count * 3);
        if hard_evict || score(&updated, &self.weights) < self.min_score {
            return Ok(()); // already removed, stays dropped
        }

        let s = score(&updated, &self.weights);
        let member = serde_json::to_string(&updated)?;
        let mut conn = self.conn();
        let _: () = conn.zadd(redis_key(&updated.protocol), &member, s).await?;
        Ok(())
    }

    /// Mark a proxy as successful; refresh score.
    pub async fn mark_success(&self, proxy: &Proxy) -> anyhow::Result<()> {
        self.remove_existing(&proxy.protocol, proxy).await?;
        let mut updated = proxy.clone();
        updated.success_count += 1;
        let s = score(&updated, &self.weights);
        let member = serde_json::to_string(&updated)?;
        let mut conn = self.conn();
        let _: () = conn.zadd(redis_key(&updated.protocol), &member, s).await?;
        Ok(())
    }

    /// Mark a proxy as failed and update circuit breaker state.
    ///
    /// If the net failure count exceeds the circuit breaker threshold,
    /// the proxy is tripped (circuit opened). Otherwise, the proxy is
    /// updated with incremented fail_count and re-scored.
    pub async fn mark_failed_with_circuit(&self, proxy: &Proxy) -> anyhow::Result<()> {
        self.remove_existing(&proxy.protocol, proxy).await?;
        let mut updated = proxy.clone();
        updated.fail_count += 1;

        // Check circuit breaker
        if circuit::should_trip(&updated, &self.circuit_config) {
            updated = circuit::trip(&updated, &self.circuit_config);
            tracing::info!("circuit tripped for {}", updated.key());
        }

        // Hard eviction: too many failures
        let hard_evict = updated.fail_count > std::cmp::max(8, updated.success_count * 3);
        if hard_evict || score(&updated, &self.weights) < self.min_score {
            return Ok(()); // already removed, stays dropped
        }

        let s = score(&updated, &self.weights);
        let member = serde_json::to_string(&updated)?;
        let mut conn = self.conn();
        let _: () = conn.zadd(redis_key(&updated.protocol), &member, s).await?;
        Ok(())
    }

    /// Mark a proxy as successful and reset circuit breaker if half-open.
    ///
    /// If the proxy was in half-open state, this resets the circuit to closed.
    pub async fn mark_success_with_circuit(&self, proxy: &Proxy) -> anyhow::Result<()> {
        self.remove_existing(&proxy.protocol, proxy).await?;
        let mut updated = proxy.clone();
        updated.success_count += 1;

        // Reset circuit breaker if it was half-open
        if circuit::is_half_open(&updated) {
            updated = circuit::reset(&updated);
            tracing::info!("circuit reset for {} — back to closed", updated.key());
        }

        let s = score(&updated, &self.weights);
        let member = serde_json::to_string(&updated)?;
        let mut conn = self.conn();
        let _: () = conn.zadd(redis_key(&updated.protocol), &member, s).await?;
        Ok(())
    }

    /// Remove a specific proxy from the store (matched by host + port + protocol).
    ///
    /// Returns `true` if the proxy was found and removed, `false` if not found.
    pub async fn remove(&self, proxy: &Proxy) -> anyhow::Result<bool> {
        self.remove_existing(&proxy.protocol, proxy).await
    }

    /// Get all proxies for a protocol, sorted by score (highest first).
    pub async fn all(&self, protocol: Protocol) -> anyhow::Result<Vec<Proxy>> {
        let key = redis_key(&protocol);
        let mut conn = self.conn();
        let members: Vec<String> = conn.zrevrange(&key, 0, -1).await?;
        let mut result = Vec::with_capacity(members.len());
        for m in members {
            match serde_json::from_str::<Proxy>(&m) {
                Ok(p) => result.push(p),
                Err(e) => tracing::warn!("failed to parse proxy from redis: {e}"),
            }
        }
        Ok(result)
    }

    /// Count proxies for a protocol.
    pub async fn count(&self, protocol: Protocol) -> anyhow::Result<usize> {
        let key = redis_key(&protocol);
        let mut conn = self.conn();
        let c: u64 = conn.zcard(&key).await?;
        Ok(c as usize)
    }

    /// Remove any stored member matching this proxy's host:port:protocol.
    ///
    /// Returns `true` if at least one member was removed.
    async fn remove_existing(&self, protocol: &Protocol, proxy: &Proxy) -> anyhow::Result<bool> {
        let key = redis_key(protocol);
        let mut conn = self.conn();
        let members: Vec<String> = conn.zrange(&key, 0, -1).await?;
        let mut found = false;
        for m in members {
            if let Ok(stored) = serde_json::from_str::<Proxy>(&m)
                && stored.host == proxy.host
                && stored.port == proxy.port
                && stored.protocol == *protocol
            {
                let _: () = conn.zrem(&key, &m).await?;
                found = true;
            }
        }
        Ok(found)
    }

    // -----------------------------------------------------------------------
    // Filtered query methods
    // -----------------------------------------------------------------------

    /// Query proxies with a composite filter, sorted by score descending.
    ///
    /// Applies all non-`None` fields of `filter` and returns up to `limit`
    /// matching proxies.
    pub async fn query(
        &self,
        protocol: Protocol,
        filter: &ProxyFilter,
        limit: usize,
    ) -> anyhow::Result<Vec<Proxy>> {
        let all = self.all(protocol).await?;
        let filtered = apply_filter(all, filter, &self.weights);
        Ok(filtered.into_iter().take(limit).collect())
    }

    /// Get the highest-scored proxy matching the filter.
    ///
    /// If no proxy matches, returns `Ok(None)`.
    pub async fn get_best_filtered(
        &self,
        protocol: Protocol,
        filter: &ProxyFilter,
    ) -> anyhow::Result<Option<Proxy>> {
        let all = self.all(protocol).await?;
        let filtered = apply_filter(all, filter, &self.weights);
        Ok(filtered.into_iter().next())
    }

    /// Get a random proxy matching the filter (weighted by score).
    ///
    /// If no proxy matches, returns `Ok(None)`.
    pub async fn get_random_filtered(
        &self,
        protocol: Protocol,
        filter: &ProxyFilter,
    ) -> anyhow::Result<Option<Proxy>> {
        let all = self.all(protocol).await?;
        let filtered = apply_filter(all, filter, &self.weights);
        if filtered.is_empty() {
            return Ok(None);
        }
        let score_fn = |p: &Proxy| score(p, &self.weights);
        Ok(weighted_random_choice(&filtered, score_fn))
    }
}

// ---------------------------------------------------------------------------
// Filter logic
// ---------------------------------------------------------------------------

/// Apply a composite filter to a list of proxies.
///
/// Each `Some` field in `filter` acts as a constraint; `None` fields are
/// ignored. The `weights` are required for `min_score` filtering.
fn apply_filter(proxies: Vec<Proxy>, filter: &ProxyFilter, weights: &ScoreWeights) -> Vec<Proxy> {
    if filter.is_empty() {
        return proxies;
    }
    proxies
        .into_iter()
        .filter(|p| {
            if let Some(ref country) = filter.country
                && p.country.as_deref() != Some(country.as_str())
            {
                return false;
            }
            if let Some(ref min_anon) = filter.anonymity {
                let min_level =
                    Anonymity::from_str_loose(min_anon).unwrap_or(Anonymity::Transparent);
                match p.anonymity {
                    Some(a) if a.meets(min_level) => {}
                    _ => return false,
                }
            }
            if let Some(max_lat) = filter.max_latency
                && p.latency_ms.is_none_or(|l| l > max_lat)
            {
                return false;
            }
            if let Some(overseas) = filter.overseas
                && p.is_overseas != overseas
            {
                return false;
            }
            if let Some(min_score) = filter.min_score
                && score(p, weights) < min_score
            {
                return false;
            }
            if let Some(ref source) = filter.source
                && p.source.as_deref() != Some(source.as_str())
            {
                return false;
            }
            if filter.alive == Some(true) && p.circuit_open {
                return false;
            }
            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_weights() -> ScoreWeights {
        ScoreWeights {
            latency: 0.5,
            success: 0.3,
            anonymity: 0.2,
        }
    }

    fn make_proxy(host: &str, port: u16) -> Proxy {
        Proxy::new(host, port, Protocol::Http)
    }

    // -- apply_filter: empty filter passes all --

    #[test]
    fn empty_filter_returns_all() {
        let proxies = vec![make_proxy("1.1.1.1", 80), make_proxy("2.2.2.2", 8080)];
        let filter = ProxyFilter::default();
        let result = apply_filter(proxies, &filter, &default_weights());
        assert_eq!(result.len(), 2);
    }

    // -- apply_filter: country --

    #[test]
    fn country_filter_exact_match() {
        let mut p = make_proxy("1.1.1.1", 80);
        p.country = Some("US".into());
        let p2 = make_proxy("2.2.2.2", 8080); // no country
        let filter = ProxyFilter {
            country: Some("US".into()),
            ..Default::default()
        };
        let result = apply_filter(vec![p, p2], &filter, &default_weights());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].host, "1.1.1.1");
    }

    // -- apply_filter: anonymity --

    #[test]
    fn anonymity_filter_elite_excludes_anonymous() {
        let mut p1 = make_proxy("1.1.1.1", 80);
        p1.anonymity = Some(Anonymity::Elite);
        let mut p2 = make_proxy("2.2.2.2", 8080);
        p2.anonymity = Some(Anonymity::Anonymous);
        let p3 = make_proxy("3.3.3.3", 9090); // no anonymity data
        let filter = ProxyFilter {
            anonymity: Some("elite".into()),
            ..Default::default()
        };
        let result = apply_filter(vec![p1, p2, p3], &filter, &default_weights());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].host, "1.1.1.1");
    }

    #[test]
    fn anonymity_filter_anonymous_includes_elite() {
        let mut p1 = make_proxy("1.1.1.1", 80);
        p1.anonymity = Some(Anonymity::Elite);
        let mut p2 = make_proxy("2.2.2.2", 8080);
        p2.anonymity = Some(Anonymity::Anonymous);
        let mut p3 = make_proxy("3.3.3.3", 9090);
        p3.anonymity = Some(Anonymity::Transparent);
        let filter = ProxyFilter {
            anonymity: Some("anonymous".into()),
            ..Default::default()
        };
        let result = apply_filter(vec![p1, p2, p3], &filter, &default_weights());
        assert_eq!(result.len(), 2); // elite + anonymous
    }

    // -- apply_filter: max_latency --

    #[test]
    fn max_latency_excludes_slow_and_unknown() {
        let mut p1 = make_proxy("1.1.1.1", 80);
        p1.latency_ms = Some(100.0);
        let mut p2 = make_proxy("2.2.2.2", 8080);
        p2.latency_ms = Some(600.0);
        let p3 = make_proxy("3.3.3.3", 9090); // no latency data
        let filter = ProxyFilter {
            max_latency: Some(500.0),
            ..Default::default()
        };
        let result = apply_filter(vec![p1, p2, p3], &filter, &default_weights());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].host, "1.1.1.1");
    }

    // -- apply_filter: overseas --

    #[test]
    fn overseas_filter() {
        let mut p1 = make_proxy("1.1.1.1", 80);
        p1.is_overseas = true;
        let mut p2 = make_proxy("2.2.2.2", 8080);
        p2.is_overseas = false;
        let filter = ProxyFilter {
            overseas: Some(true),
            ..Default::default()
        };
        let result = apply_filter(vec![p1, p2], &filter, &default_weights());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].host, "1.1.1.1");
    }

    // -- apply_filter: alive (circuit breaker) --

    #[test]
    fn alive_excludes_circuit_open() {
        let mut p1 = make_proxy("1.1.1.1", 80);
        p1.circuit_open = false;
        let mut p2 = make_proxy("2.2.2.2", 8080);
        p2.circuit_open = true;
        let filter = ProxyFilter {
            alive: Some(true),
            ..Default::default()
        };
        let result = apply_filter(vec![p1, p2], &filter, &default_weights());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].host, "1.1.1.1");
    }

    // -- apply_filter: source --

    #[test]
    fn source_filter_exact_match() {
        let mut p1 = make_proxy("1.1.1.1", 80);
        p1.source = Some("fate0".into());
        let mut p2 = make_proxy("2.2.2.2", 8080);
        p2.source = Some("other".into());
        let filter = ProxyFilter {
            source: Some("fate0".into()),
            ..Default::default()
        };
        let result = apply_filter(vec![p1, p2], &filter, &default_weights());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].host, "1.1.1.1");
    }

    // -- apply_filter: min_score --

    #[test]
    fn min_score_filter() {
        let mut p1 = make_proxy("1.1.1.1", 80);
        p1.latency_ms = Some(50.0);
        p1.success_count = 10;
        p1.anonymity = Some(Anonymity::Elite);
        let mut p2 = make_proxy("2.2.2.2", 8080);
        p2.latency_ms = Some(1500.0);
        p2.fail_count = 5;
        let filter = ProxyFilter {
            min_score: Some(0.5),
            ..Default::default()
        };
        let result = apply_filter(vec![p1, p2], &filter, &default_weights());
        assert!(!result.is_empty());
        assert_eq!(result[0].host, "1.1.1.1");
    }

    // -- apply_filter: combined filters --

    #[test]
    fn combined_country_and_alive() {
        let mut p1 = make_proxy("1.1.1.1", 80);
        p1.country = Some("US".into());
        p1.circuit_open = false;
        let mut p2 = make_proxy("2.2.2.2", 8080);
        p2.country = Some("US".into());
        p2.circuit_open = true;
        let mut p3 = make_proxy("3.3.3.3", 9090);
        p3.country = Some("JP".into());
        p3.circuit_open = false;
        let filter = ProxyFilter {
            country: Some("US".into()),
            alive: Some(true),
            ..Default::default()
        };
        let result = apply_filter(vec![p1, p2, p3], &filter, &default_weights());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].host, "1.1.1.1");
    }

    // -- score function --

    #[test]
    fn score_untested_proxy_is_neutral() {
        let p = make_proxy("1.1.1.1", 80);
        let s = score(&p, &default_weights());
        // latency=5000→0.0, success_rate=0.5, anonymity=0.0
        // 0.5*0.0 + 0.3*0.5 + 0.2*0.0 = 0.15
        assert!((s - 0.15).abs() < 0.001);
    }

    #[test]
    fn score_fast_elite_proxy() {
        let mut p = make_proxy("1.1.1.1", 80);
        p.latency_ms = Some(50.0);
        p.success_count = 10;
        p.anonymity = Some(Anonymity::Elite);
        let s = score(&p, &default_weights());
        assert!(s > 0.8);
    }
}
