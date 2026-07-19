//! Redis-backed proxy storage with ZSet scoring per protocol.

use crate::circuit::{self, CircuitBreakerConfig};
use crate::config::ScoreWeights;
use crate::models::{Anonymity, Protocol, Proxy, ProxyFilter, QualityTrend};
use redis::AsyncCommands;
use redis::aio::MultiplexedConnection;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Per-factor contribution to a proxy score.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreComponent {
    /// Normalized value before weighting.
    pub normalized: f64,
    /// Configured weight for this component.
    pub weight: f64,
    /// Weighted contribution to the final score.
    pub contribution: f64,
}

/// Latency score details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LatencyScoreComponent {
    /// Raw latency in milliseconds. `None` means the proxy has not been checked.
    pub latency_ms: Option<f64>,
    #[serde(flatten)]
    pub component: ScoreComponent,
}

/// Success/failure score details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SuccessScoreComponent {
    pub success_count: u32,
    pub fail_count: u32,
    /// Normalized success rate used by the score formula.
    pub success_rate: f64,
    #[serde(flatten)]
    pub component: ScoreComponent,
}

/// Anonymity score details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnonymityScoreComponent {
    pub anonymity: Option<Anonymity>,
    #[serde(flatten)]
    pub component: ScoreComponent,
}

/// Retention decision implied by the current score policy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetentionDecision {
    Keep,
    BelowMinScore,
    HardFailureEvict,
}

/// Serializable explanation of the score and retention decision for a proxy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreExplanation {
    pub score: f64,
    pub min_score: f64,
    pub latency: LatencyScoreComponent,
    pub success: SuccessScoreComponent,
    pub anonymity: AnonymityScoreComponent,
    pub trend: QualityTrend,
    pub retention: RetentionDecision,
}

/// A proxy paired with its score explanation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredProxy {
    pub proxy: Proxy,
    pub score: ScoreExplanation,
}

/// Result of a low-score cleanup scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupLowScoreResult {
    pub protocol: Protocol,
    pub scanned: usize,
    pub eligible: usize,
    pub removed: usize,
    pub applied: bool,
    pub min_score: f64,
    pub candidates: Vec<ScoredProxy>,
}

/// Compute a 0..1 score from latency, success rate, anonymity.
pub fn score(proxy: &Proxy, weights: &ScoreWeights) -> f64 {
    score_parts(proxy, weights).score
}

/// Explain the score and retention decision for a proxy.
pub fn explain_score(proxy: &Proxy, weights: &ScoreWeights, min_score: f64) -> ScoreExplanation {
    let parts = score_parts(proxy, weights);
    let retention = retention_decision(proxy, parts.score, min_score);
    ScoreExplanation {
        score: parts.score,
        min_score,
        latency: parts.latency,
        success: parts.success,
        anonymity: parts.anonymity,
        trend: proxy.quality_history.trend(),
        retention,
    }
}

fn record_success_sample(proxy: &mut Proxy) {
    let checked_at = proxy.last_check.unwrap_or_else(chrono::Utc::now);
    proxy
        .quality_history
        .record_success(checked_at, proxy.latency_ms);
}

fn record_failure_sample(proxy: &mut Proxy, error: &str) {
    proxy
        .quality_history
        .record_failure(chrono::Utc::now(), error);
}

/// Merge accumulated validation history from an existing pool record into an
/// incoming proxy when the incoming one carries no history of its own.
///
/// Subscription/fetch refresh re-adds already-known proxies with zeroed
/// counters; without this carry-forward every refresh cycle would wipe scores,
/// quality history, and circuit state (B3). A genuinely-validated incoming
/// proxy (non-zero counts or a `last_check`) keeps its own fresh stats.
///
/// Auth credentials (`username`/`password`) are intentionally NOT carried from
/// `existing`: `merged` starts as `incoming.clone()`, so hourly-rotating
/// subscription credentials always win over stale stored ones.
fn carry_forward_history(incoming: &Proxy, existing: Option<&Proxy>) -> Proxy {
    let mut merged = incoming.clone();
    let incoming_has_history =
        incoming.last_check.is_some() || incoming.success_count > 0 || incoming.fail_count > 0;
    if incoming_has_history {
        return merged;
    }
    let Some(prev) = existing else {
        return merged;
    };
    merged.success_count = prev.success_count;
    merged.fail_count = prev.fail_count;
    merged.last_check = prev.last_check;
    merged.latency_ms = prev.latency_ms;
    merged.anonymity = merged.anonymity.or(prev.anonymity);
    merged.quality_history = prev.quality_history.clone();
    merged.circuit_open = prev.circuit_open;
    merged.circuit_open_until = prev.circuit_open_until;
    merged.country = merged.country.or_else(|| prev.country.clone());
    merged.country_name = merged.country_name.or_else(|| prev.country_name.clone());
    merged.is_overseas = prev.is_overseas || merged.is_overseas;
    merged.warp_chain_ok = prev.warp_chain_ok;
    merged.warp_chain_latency_ms = prev.warp_chain_latency_ms;
    merged.warp_chain_last_test = prev.warp_chain_last_test;
    merged
}

fn retention_decision(proxy: &Proxy, score: f64, min_score: f64) -> RetentionDecision {
    if hard_failure_evict(proxy) {
        RetentionDecision::HardFailureEvict
    } else if score < min_score {
        RetentionDecision::BelowMinScore
    } else {
        RetentionDecision::Keep
    }
}

fn hard_failure_evict(proxy: &Proxy) -> bool {
    proxy.fail_count > std::cmp::max(5, proxy.success_count * 2)
}

struct ScoreParts {
    score: f64,
    latency: LatencyScoreComponent,
    success: SuccessScoreComponent,
    anonymity: AnonymityScoreComponent,
}

fn component(normalized: f64, weight: f64) -> ScoreComponent {
    ScoreComponent {
        normalized,
        weight,
        contribution: weight * normalized,
    }
}

/// Piecewise-linear latency normalization with extended tail.
///
/// | Latency range | Norm value | Rationale |
/// |---------------|-----------|-----------|
/// | 0–1000 ms     | 1.0       | Excellent — full score |
/// | 1000–2000 ms  | 1.0→0.5   | Good — linear drop |
/// | 2000–5000 ms  | 0.5→0.1   | Fair — slower decay |
/// | 5000–10000 ms | 0.1→0.0   | Poor — long tail |
/// | >10000 ms     | 0.0       | Dead — zero |
///
/// Unlike the old `clamp((2000-ms)/2000, 0, 1)` which saturated at 2 s,
/// this curve keeps 2 s and 11 s distinguishable (0.5 vs 0.0).
pub fn latency_norm_piecewise(ms: f64) -> f64 {
    if ms <= 1000.0 {
        1.0
    } else if ms <= 2000.0 {
        1.0 - 0.5 * (ms - 1000.0) / 1000.0
    } else if ms <= 5000.0 {
        0.5 - 0.4 * (ms - 2000.0) / 3000.0
    } else if ms <= 10000.0 {
        0.1 - 0.1 * (ms - 5000.0) / 5000.0
    } else {
        0.0
    }
}

fn score_parts(proxy: &Proxy, weights: &ScoreWeights) -> ScoreParts {
    let latency = proxy.latency_ms.unwrap_or(5000.0);
    // Piecewise-linear latency normalization with extended tail.
    let latency_norm = latency_norm_piecewise(latency);

    let total = proxy.success_count + proxy.fail_count;
    let success_rate = if total == 0 {
        0.5 // untested: neutral
    } else {
        ((proxy.success_count as f64 - proxy.fail_count as f64) / total as f64).clamp(0.0, 1.0)
    };

    let anonymity_norm = proxy.anonymity.map(|a| a.bonus()).unwrap_or(0.0);
    let latency_component = component(latency_norm, weights.latency);
    let success_component = component(success_rate, weights.success);
    let anonymity_component = component(anonymity_norm, weights.anonymity);

    ScoreParts {
        score: latency_component.contribution
            + success_component.contribution
            + anonymity_component.contribution,
        latency: LatencyScoreComponent {
            latency_ms: proxy.latency_ms,
            component: latency_component,
        },
        success: SuccessScoreComponent {
            success_count: proxy.success_count,
            fail_count: proxy.fail_count,
            success_rate,
            component: success_component,
        },
        anonymity: AnonymityScoreComponent {
            anonymity: proxy.anonymity,
            component: anonymity_component,
        },
    }
}

/// Weighted random choice: prefer higher-scored proxies.
pub fn weighted_random_choice(
    proxies: &[Proxy],
    score_fn: impl Fn(&Proxy) -> f64,
) -> Option<Proxy> {
    weighted_random_index(proxies, &score_fn).map(|idx| proxies[idx].clone())
}

/// Weighted random choices without replacement: prefer higher-scored proxies.
pub fn weighted_random_choices(
    proxies: &[Proxy],
    limit: usize,
    score_fn: impl Fn(&Proxy) -> f64,
) -> Vec<Proxy> {
    if limit == 0 || proxies.is_empty() {
        return Vec::new();
    }

    let mut remaining = proxies.to_vec();
    let mut selected = Vec::with_capacity(limit.min(proxies.len()));
    while selected.len() < limit && !remaining.is_empty() {
        let Some(idx) = weighted_random_index(&remaining, &score_fn) else {
            break;
        };
        selected.push(remaining.swap_remove(idx));
    }
    selected
}

fn weighted_random_index(proxies: &[Proxy], score_fn: &impl Fn(&Proxy) -> f64) -> Option<usize> {
    if proxies.is_empty() {
        return None;
    }
    let scores: Vec<f64> = proxies.iter().map(&score_fn).collect();
    let total: f64 = scores.iter().sum();
    if total <= 0.0 {
        // All zero scores: uniform random
        let idx = (rand::random::<u64>() as usize) % proxies.len();
        return Some(idx);
    }
    let mut r = rand::random::<f64>() * total;
    for (i, s) in scores.iter().enumerate() {
        r -= s;
        if r <= 0.0 {
            return Some(i);
        }
    }
    Some(proxies.len() - 1)
}

/// Restrict a score-descending candidate list to its top slice, then draw
/// `limit` weighted-random picks from that slice.
///
/// `sorted_desc` must already be ordered by descending score (as returned by a
/// Redis `ZREVRANGE`). At least `limit` candidates are retained even when
/// `top_k` is smaller.
fn top_candidates(
    mut sorted_desc: Vec<Proxy>,
    top_k: usize,
    limit: usize,
    score_fn: impl Fn(&Proxy) -> f64,
) -> Vec<Proxy> {
    sorted_desc.truncate(top_k.max(limit));
    weighted_random_choices(&sorted_desc, limit, score_fn)
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

    /// Return a cloned Redis connection for ad-hoc key access (e.g. airport account persistence).
    pub fn raw_conn(&self) -> MultiplexedConnection {
        self.conn()
    }

    /// TTL (seconds) for gateway free_pool / xray failure cooldowns (process map + Redis).
    pub const GATEWAY_FAILURE_COOLDOWN_SECS: u64 = 300;

    /// Redis key for a free_pool gateway cooldown entry.
    pub fn gateway_proxy_cooldown_key(dedup_key: &str) -> String {
        format!("gateway:cooldown:proxy:{dedup_key}")
    }

    /// Redis key for an xray local-port gateway cooldown entry.
    pub fn gateway_xray_cooldown_key(port: u16) -> String {
        format!("gateway:cooldown:xray:{port}")
    }

    /// Put free_pool proxy into gateway failure cooldown (short TTL, not score/circuit).
    pub async fn put_gateway_proxy_cooldown(
        &self,
        dedup_key: &str,
        ttl_secs: u64,
    ) -> anyhow::Result<()> {
        let key = Self::gateway_proxy_cooldown_key(dedup_key);
        let mut conn = self.conn();
        conn.set_ex::<_, _, ()>(key, "1", ttl_secs).await?;
        Ok(())
    }

    /// Clear free_pool gateway failure cooldown.
    pub async fn clear_gateway_proxy_cooldown(&self, dedup_key: &str) -> anyhow::Result<()> {
        let key = Self::gateway_proxy_cooldown_key(dedup_key);
        let mut conn = self.conn();
        let _: () = conn.del(key).await?;
        Ok(())
    }

    /// Whether free_pool proxy is under gateway failure cooldown.
    pub async fn is_gateway_proxy_cooling_down(&self, dedup_key: &str) -> anyhow::Result<bool> {
        let key = Self::gateway_proxy_cooldown_key(dedup_key);
        let mut conn = self.conn();
        let n: i64 = conn.exists(key).await?;
        Ok(n > 0)
    }

    /// Put xray local SOCKS5 port into gateway failure cooldown.
    pub async fn put_gateway_xray_cooldown(&self, port: u16, ttl_secs: u64) -> anyhow::Result<()> {
        let key = Self::gateway_xray_cooldown_key(port);
        let mut conn = self.conn();
        conn.set_ex::<_, _, ()>(key, "1", ttl_secs).await?;
        Ok(())
    }

    /// Clear xray gateway failure cooldown.
    pub async fn clear_gateway_xray_cooldown(&self, port: u16) -> anyhow::Result<()> {
        let key = Self::gateway_xray_cooldown_key(port);
        let mut conn = self.conn();
        let _: () = conn.del(key).await?;
        Ok(())
    }

    /// Whether xray local port is under gateway failure cooldown.
    pub async fn is_gateway_xray_cooling_down(&self, port: u16) -> anyhow::Result<bool> {
        let key = Self::gateway_xray_cooldown_key(port);
        let mut conn = self.conn();
        let n: i64 = conn.exists(key).await?;
        Ok(n > 0)
    }

    /// Add a proxy to the store (upsert by dedup key).
    ///
    /// Removes any existing entry for the same logical proxy (host:port:protocol)
    /// before inserting, so that stale stats don't create duplicate ZSET members.
    pub async fn add(&self, proxy: &Proxy) -> anyhow::Result<()> {
        let existing = self.take_existing(&proxy.protocol, proxy).await?;
        let mut merged = carry_forward_history(proxy, existing.as_ref());

        // Only record a sample for a genuinely-validated incoming proxy; keying
        // this on the original input (not `merged`) avoids adding a spurious
        // sample when a carried-forward `last_check` is present.
        if proxy.last_check.is_some() {
            record_success_sample(&mut merged);
        }
        let s = score(&merged, &self.weights);
        let member = serde_json::to_string(&merged)?;
        let key = redis_key(&merged.protocol);
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

    /// Get multiple weighted-random proxies without replacement.
    pub async fn get_random_candidates(
        &self,
        protocol: Protocol,
        limit: usize,
    ) -> anyhow::Result<Vec<Proxy>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let key = redis_key(&protocol);
        let mut conn = self.conn();
        let members: Vec<String> = conn.zrevrange(&key, 0, -1).await?;
        if members.is_empty() {
            return Ok(Vec::new());
        }
        let proxies: Vec<Proxy> = members
            .iter()
            .filter_map(|m| serde_json::from_str::<Proxy>(m).ok())
            .filter(|p| !circuit::is_circuit_open(p))
            .collect();
        if proxies.is_empty() {
            return Ok(Vec::new());
        }
        let score_fn = |p: &Proxy| score(p, &self.weights);
        Ok(weighted_random_choices(&proxies, limit, score_fn))
    }

    /// Get weighted-random proxies drawn only from the top-`top_k` highest-scored
    /// (non-circuit-open) entries.
    ///
    /// Selecting from the whole pool lets a large mass of low-score proxies
    /// dominate the weighted draw; restricting to the top slice keeps the
    /// gateway biased toward proxies that actually work while still spreading
    /// load across several of them.
    pub async fn get_top_candidates(
        &self,
        protocol: Protocol,
        top_k: usize,
        limit: usize,
    ) -> anyhow::Result<Vec<Proxy>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let key = redis_key(&protocol);
        let mut conn = self.conn();
        // zrevrange returns members in descending stored-score order.
        let members: Vec<String> = conn.zrevrange(&key, 0, -1).await?;
        let proxies: Vec<Proxy> = members
            .iter()
            .filter_map(|m| serde_json::from_str::<Proxy>(m).ok())
            .filter(|p| !circuit::is_circuit_open(p))
            .collect();
        if proxies.is_empty() {
            return Ok(Vec::new());
        }
        let score_fn = |p: &Proxy| score(p, &self.weights);
        Ok(top_candidates(proxies, top_k, limit, score_fn))
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
    pub async fn mark_failed(&self, proxy: &Proxy, reason: &str) -> anyhow::Result<()> {
        self.remove_existing(&proxy.protocol, proxy).await?;
        let mut updated = proxy.clone();
        updated.fail_count += 1;
        record_failure_sample(&mut updated, reason);

        // Hard eviction: too many failures
        if hard_failure_evict(&updated) || score(&updated, &self.weights) < self.min_score {
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
        record_success_sample(&mut updated);
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
    pub async fn mark_failed_with_circuit(
        &self,
        proxy: &Proxy,
        reason: &str,
    ) -> anyhow::Result<()> {
        self.remove_existing(&proxy.protocol, proxy).await?;
        let mut updated = proxy.clone();
        updated.fail_count += 1;
        record_failure_sample(&mut updated, reason);

        // Check circuit breaker
        if circuit::should_trip(&updated, &self.circuit_config) {
            updated = circuit::trip(&updated, &self.circuit_config);
            tracing::info!("circuit tripped for {}", updated.key());
        }

        // Hard eviction: too many failures
        if hard_failure_evict(&updated) || score(&updated, &self.weights) < self.min_score {
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
        record_success_sample(&mut updated);

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

    /// Explain how this store scores a proxy with its configured policy.
    pub fn explain(&self, proxy: &Proxy) -> ScoreExplanation {
        explain_score(proxy, &self.weights, self.min_score)
    }

    /// Query proxies and attach score explanations.
    pub async fn query_scored(
        &self,
        protocol: Protocol,
        filter: &ProxyFilter,
        limit: usize,
    ) -> anyhow::Result<Vec<ScoredProxy>> {
        let proxies = self.query(protocol, filter, limit).await?;
        Ok(proxies
            .into_iter()
            .map(|proxy| ScoredProxy {
                score: self.explain(&proxy),
                proxy,
            })
            .collect())
    }

    /// Scan low-score proxies and optionally remove them.
    pub async fn cleanup_low_score(
        &self,
        protocol: Protocol,
        limit: usize,
        min_score: Option<f64>,
        apply: bool,
    ) -> anyhow::Result<CleanupLowScoreResult> {
        let threshold = min_score.unwrap_or(self.min_score);
        let scanned_proxies: Vec<Proxy> =
            self.all(protocol).await?.into_iter().take(limit).collect();
        let scanned = scanned_proxies.len();
        let candidates: Vec<ScoredProxy> = scanned_proxies
            .into_iter()
            .filter_map(|proxy| {
                let score = explain_score(&proxy, &self.weights, threshold);
                (score.retention != RetentionDecision::Keep).then_some(ScoredProxy { proxy, score })
            })
            .collect();

        let mut removed = 0;
        if apply {
            for candidate in &candidates {
                if self.remove(&candidate.proxy).await? {
                    removed += 1;
                }
            }
        }

        Ok(CleanupLowScoreResult {
            protocol,
            scanned,
            eligible: candidates.len(),
            removed,
            applied: apply,
            min_score: threshold,
            candidates,
        })
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

    /// Remove all stored members matching this proxy's host:port:protocol and
    /// return the richest matched record (most validation samples), if any.
    ///
    /// Used by `add` to carry accumulated history forward across re-adds.
    async fn take_existing(
        &self,
        protocol: &Protocol,
        proxy: &Proxy,
    ) -> anyhow::Result<Option<Proxy>> {
        let key = redis_key(protocol);
        let mut conn = self.conn();
        let members: Vec<String> = conn.zrange(&key, 0, -1).await?;
        let mut richest: Option<Proxy> = None;
        for m in members {
            if let Ok(stored) = serde_json::from_str::<Proxy>(&m)
                && stored.host == proxy.host
                && stored.port == proxy.port
                && stored.protocol == *protocol
            {
                let _: () = conn.zrem(&key, &m).await?;
                let samples = stored.success_count + stored.fail_count;
                let keep = match &richest {
                    Some(cur) => samples >= cur.success_count + cur.fail_count,
                    None => true,
                };
                if keep {
                    richest = Some(stored);
                }
            }
        }
        Ok(richest)
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

    #[test]
    fn carry_forward_history_preserves_stats_on_fresh_readd() {
        // Simulate a subscription refresh re-adding an already-validated proxy
        // with a fresh, zeroed record.
        let mut existing = make_proxy("1.1.1.1", 80);
        existing.success_count = 12;
        existing.fail_count = 3;
        existing.latency_ms = Some(150.0);
        existing.last_check = Some(chrono::Utc::now());
        existing.circuit_open = true;
        existing
            .quality_history
            .record_success(chrono::Utc::now(), Some(150.0));

        let mut incoming = make_proxy("1.1.1.1", 80);
        incoming.source = Some("subscription:https://sub.example".into());

        let merged = carry_forward_history(&incoming, Some(&existing));
        assert_eq!(merged.success_count, 12);
        assert_eq!(merged.fail_count, 3);
        assert_eq!(merged.latency_ms, Some(150.0));
        assert!(merged.last_check.is_some());
        assert!(merged.circuit_open);
        assert!(!merged.quality_history.is_empty());
        // Incoming identity/source is kept.
        assert_eq!(
            merged.source.as_deref(),
            Some("subscription:https://sub.example")
        );
    }

    #[test]
    fn carry_forward_history_keeps_validated_incoming_stats() {
        let mut existing = make_proxy("1.1.1.1", 80);
        existing.success_count = 12;
        existing.fail_count = 0;

        // A freshly-validated incoming proxy carries its own history and must win.
        let mut incoming = make_proxy("1.1.1.1", 80);
        incoming.success_count = 1;
        incoming.fail_count = 4;
        incoming.last_check = Some(chrono::Utc::now());

        let merged = carry_forward_history(&incoming, Some(&existing));
        assert_eq!(merged.success_count, 1);
        assert_eq!(merged.fail_count, 4);
    }

    #[test]
    fn carry_forward_history_keeps_incoming_credentials() {
        // Hourly-rotating subscription credentials must replace stale ones.
        let mut existing = make_proxy("1.1.1.1", 80);
        existing.success_count = 5;
        existing.username = Some("old-user".into());
        existing.password = Some("old-pass".into());

        let mut incoming = make_proxy("1.1.1.1", 80);
        incoming.username = Some("new-user".into());
        incoming.password = Some("new-pass".into());

        let merged = carry_forward_history(&incoming, Some(&existing));
        assert_eq!(merged.credentials(), Some(("new-user", "new-pass")));
        // History still carries forward when incoming has none.
        assert_eq!(merged.success_count, 5);
    }

    #[test]
    fn weighted_random_choices_respects_limit_and_no_replacement() {
        let proxies = vec![
            make_proxy("1.1.1.1", 80),
            make_proxy("2.2.2.2", 8080),
            make_proxy("3.3.3.3", 9090),
        ];

        let selected = weighted_random_choices(&proxies, 2, |_| 1.0);

        assert_eq!(selected.len(), 2);
        assert_ne!(selected[0].dedup_key(), selected[1].dedup_key());
    }

    #[test]
    fn top_candidates_only_draws_from_top_slice() {
        // 10 proxies in descending-score order; only the first 3 are eligible
        // when top_k = 3. Every draw must come from that slice.
        let sorted_desc: Vec<Proxy> = (0..10).map(|i| make_proxy("10.0.0.1", 1000 + i)).collect();
        let allowed: std::collections::HashSet<String> =
            sorted_desc[..3].iter().map(|p| p.dedup_key()).collect();

        for _ in 0..50 {
            let picked = top_candidates(sorted_desc.clone(), 3, 2, |_| 1.0);
            assert_eq!(picked.len(), 2);
            for p in &picked {
                assert!(
                    allowed.contains(&p.dedup_key()),
                    "picked {} outside top-3 slice",
                    p.dedup_key()
                );
            }
        }
    }

    #[test]
    fn top_candidates_keeps_at_least_limit() {
        // top_k smaller than limit must still yield `limit` picks.
        let sorted_desc: Vec<Proxy> = (0..5).map(|i| make_proxy("10.0.0.2", 2000 + i)).collect();
        let picked = top_candidates(sorted_desc, 1, 3, |_| 1.0);
        assert_eq!(picked.len(), 3);
    }

    #[test]
    fn weighted_random_choices_zero_limit_returns_empty() {
        let proxies = vec![make_proxy("1.1.1.1", 80)];

        let selected = weighted_random_choices(&proxies, 0, |_| 1.0);

        assert!(selected.is_empty());
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
        // latency=5000→0.1 (piecewise), success_rate=0.5, anonymity=0.0
        // 0.5*0.1 + 0.3*0.5 + 0.2*0.0 = 0.20
        assert!((s - 0.20).abs() < 0.001);
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

    #[test]
    fn explain_score_includes_component_contributions() {
        let mut p = make_proxy("1.1.1.1", 80);
        p.latency_ms = Some(100.0);
        p.success_count = 8;
        p.fail_count = 2;
        p.anonymity = Some(Anonymity::Anonymous);
        p.quality_history
            .record_success(chrono::Utc::now(), Some(100.0));
        p.quality_history
            .record_failure(chrono::Utc::now(), "timeout");

        let explanation = explain_score(&p, &default_weights(), 0.1);

        assert_eq!(explanation.retention, RetentionDecision::Keep);
        assert_eq!(explanation.latency.latency_ms, Some(100.0));
        assert!((explanation.latency.component.normalized - 1.0).abs() < 0.001);
        assert!((explanation.success.success_rate - 0.6).abs() < 0.001);
        assert_eq!(explanation.anonymity.anonymity, Some(Anonymity::Anonymous));
        assert!((explanation.anonymity.component.normalized - 0.5).abs() < 0.001);
        assert_eq!(explanation.trend.recent_samples, 2);
        assert_eq!(explanation.trend.recent_failures, 1);
        assert_eq!(explanation.trend.recent_latency_p50, Some(100.0));
        assert!((explanation.score - score(&p, &default_weights())).abs() < 0.001);
    }

    #[test]
    fn explain_score_marks_below_min_score() {
        let mut p = make_proxy("2.2.2.2", 8080);
        p.latency_ms = Some(3000.0);
        p.fail_count = 2;

        let explanation = explain_score(&p, &default_weights(), 0.2);

        assert_eq!(explanation.retention, RetentionDecision::BelowMinScore);
        assert!(explanation.score < explanation.min_score);
    }

    #[test]
    fn explain_score_marks_hard_failure_before_min_score() {
        let mut p = make_proxy("3.3.3.3", 9090);
        p.latency_ms = Some(50.0);
        p.success_count = 0;
        p.fail_count = 9;
        p.anonymity = Some(Anonymity::Elite);

        let explanation = explain_score(&p, &default_weights(), 0.1);

        assert_eq!(explanation.retention, RetentionDecision::HardFailureEvict);
    }

    #[test]
    fn score_explanation_serializes_retention() {
        let p = make_proxy("4.4.4.4", 8000);
        let explanation = explain_score(&p, &default_weights(), 0.1);
        let json = serde_json::to_string(&explanation).unwrap();
        assert!(json.contains("\"retention\":\"keep\""));
        assert!(json.contains("\"min_score\":0.1"));
        assert!(json.contains("\"trend\""));
        assert!(json.contains("\"recent_samples\":0"));
    }

    #[test]
    fn gateway_cooldown_redis_keys_are_namespaced() {
        assert_eq!(
            ProxyStore::gateway_proxy_cooldown_key("socks5:9.9.9.9:1080"),
            "gateway:cooldown:proxy:socks5:9.9.9.9:1080"
        );
        assert_eq!(
            ProxyStore::gateway_xray_cooldown_key(21000),
            "gateway:cooldown:xray:21000"
        );
        assert_eq!(ProxyStore::GATEWAY_FAILURE_COOLDOWN_SECS, 300);
        // Must not collide with pool ZSET keys.
        assert!(!ProxyStore::gateway_proxy_cooldown_key("http:1.1.1.1:80").starts_with("proxies:"));
    }

    #[test]
    fn record_success_sample_uses_last_check_and_latency() {
        let mut p = make_proxy("5.5.5.5", 8080);
        let checked_at = chrono::Utc::now();
        p.last_check = Some(checked_at);
        p.latency_ms = Some(123.0);

        record_success_sample(&mut p);

        let sample = p.quality_history.samples.first().unwrap();
        assert!(sample.success);
        assert_eq!(sample.checked_at_unix_secs, checked_at.timestamp());
        assert_eq!(sample.latency_ms, Some(123.0));
    }

    #[test]
    fn record_failure_sample_adds_failure_trend() {
        let mut p = make_proxy("6.6.6.6", 8080);

        record_failure_sample(&mut p, "validation_failed");

        let trend = p.quality_history.trend();
        assert_eq!(trend.recent_samples, 1);
        assert_eq!(trend.recent_failures, 1);
        assert_eq!(trend.recent_success_rate, Some(0.0));
        assert_eq!(trend.recent_latency_p50, None);
    }

    // -- latency_norm_piecewise curve tests --

    #[test]
    fn latency_norm_piecewise_excellent_tier() {
        // 0–1000 ms → 1.0
        assert!((latency_norm_piecewise(500.0) - 1.0).abs() < 0.001);
        assert!((latency_norm_piecewise(1000.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn latency_norm_piecewise_good_tier() {
        // 1000–2000 ms → 1.0→0.5
        assert!((latency_norm_piecewise(1500.0) - 0.75).abs() < 0.001);
        assert!((latency_norm_piecewise(2000.0) - 0.5).abs() < 0.001);
    }

    #[test]
    fn latency_norm_piecewise_fair_tier() {
        // 2000–5000 ms → 0.5→0.1
        assert!((latency_norm_piecewise(3000.0) - 0.3667).abs() < 0.01);
        assert!((latency_norm_piecewise(5000.0) - 0.1).abs() < 0.001);
    }

    #[test]
    fn latency_norm_piecewise_poor_tier() {
        // 5000–10000 ms → 0.1→0.0
        assert!((latency_norm_piecewise(7500.0) - 0.05).abs() < 0.001);
        assert!((latency_norm_piecewise(10000.0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn latency_norm_piecewise_dead() {
        // >10000 ms → 0.0
        assert!((latency_norm_piecewise(11000.0) - 0.0).abs() < 0.001);
        assert!((latency_norm_piecewise(60000.0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn latency_norm_piecewise_strictly_decreasing() {
        let values = [
            100.0, 500.0, 1000.0, 1200.0, 1500.0, 1800.0, 2000.0, 2500.0, 3000.0, 4000.0, 5000.0,
            7000.0, 9000.0, 10000.0, 12000.0,
        ];
        let norms: Vec<f64> = values
            .iter()
            .map(|&ms| latency_norm_piecewise(ms))
            .collect();
        for i in 1..norms.len() {
            assert!(
                norms[i] <= norms[i - 1],
                "not non-increasing at {}ms vs {}ms: {} vs {}",
                values[i - 1],
                values[i],
                norms[i - 1],
                norms[i]
            );
        }
    }

    #[test]
    fn score_ordering_by_latency() {
        let w = default_weights();
        let mut elite_500 = make_proxy("1.1.1.1", 80);
        elite_500.latency_ms = Some(500.0);
        elite_500.success_count = 10;
        elite_500.anonymity = Some(Anonymity::Elite);

        let mut elite_2s = make_proxy("2.2.2.2", 80);
        elite_2s.latency_ms = Some(2000.0);
        elite_2s.success_count = 10;
        elite_2s.anonymity = Some(Anonymity::Elite);

        let mut elite_5s = make_proxy("3.3.3.3", 80);
        elite_5s.latency_ms = Some(5000.0);
        elite_5s.success_count = 10;
        elite_5s.anonymity = Some(Anonymity::Elite);

        let mut elite_10s = make_proxy("4.4.4.4", 80);
        elite_10s.latency_ms = Some(10000.0);
        elite_10s.success_count = 10;
        elite_10s.anonymity = Some(Anonymity::Elite);

        assert!(
            score(&elite_500, &w) > score(&elite_2s, &w),
            "500ms elite should outscore 2s elite"
        );
        assert!(
            score(&elite_2s, &w) > score(&elite_5s, &w),
            "2s elite should outscore 5s elite"
        );
        assert!(
            score(&elite_5s, &w) > score(&elite_10s, &w),
            "5s elite should outscore 10s elite"
        );
    }
}
