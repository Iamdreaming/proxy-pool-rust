//! Circuit breaker for proxy health management.
//!
//! Three states: Closed (healthy) → Open (tripped) → Half-Open (probing).

use crate::models::Proxy;
use chrono::Utc;

/// Configuration for the circuit breaker.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Consecutive net failures (fail_count - success_count) to trip the circuit.
    pub failure_threshold: u32,
    /// Seconds before a tripped circuit transitions to half-open.
    pub recovery_timeout_sec: f64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            recovery_timeout_sec: 300.0,
        }
    }
}

/// Check if the proxy's circuit breaker is Open (should be excluded).
pub fn is_circuit_open(proxy: &Proxy) -> bool {
    if !proxy.circuit_open {
        return false;
    }
    let Some(until) = proxy.circuit_open_until else {
        return true; // indefinitely open
    };
    Utc::now() < until
}

/// Check if the proxy's circuit breaker is in Half-Open state.
pub fn is_half_open(proxy: &Proxy) -> bool {
    if !proxy.circuit_open {
        return false;
    }
    let Some(until) = proxy.circuit_open_until else {
        return false; // no recovery time = permanently open
    };
    Utc::now() >= until
}

/// Trip the circuit breaker on a proxy (transition to Open).
pub fn trip(proxy: &Proxy, config: &CircuitBreakerConfig) -> Proxy {
    let now = Utc::now();
    let open_until = now + chrono::Duration::seconds(config.recovery_timeout_sec as i64);
    let mut updated = proxy.clone();
    updated.circuit_open = true;
    updated.circuit_open_until = Some(open_until);
    tracing::info!(
        "circuit tripped for {} — open until {}",
        proxy.key(),
        open_until
    );
    updated
}

/// Reset the circuit breaker (transition to Closed).
pub fn reset(proxy: &Proxy) -> Proxy {
    if !proxy.circuit_open {
        return proxy.clone();
    }
    tracing::info!("circuit reset for {} — back to closed", proxy.key());
    let mut updated = proxy.clone();
    updated.circuit_open = false;
    updated.circuit_open_until = None;
    updated
}

/// Check if net failures exceed the threshold and the circuit should trip.
pub fn should_trip(proxy: &Proxy, config: &CircuitBreakerConfig) -> bool {
    let net_failures = proxy.fail_count as i64 - proxy.success_count as i64;
    net_failures >= config.failure_threshold as i64
}
