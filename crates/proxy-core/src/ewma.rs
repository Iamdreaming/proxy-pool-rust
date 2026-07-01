//! Exponentially Weighted Moving Average (EWMA) for latency smoothing.

/// Default alpha (smoothing factor): 0.3 balances responsiveness and stability.
pub const DEFAULT_ALPHA: f64 = 0.3;

/// Return the updated EWMA given the previous value and a new observation.
///
/// - `current`: previous EWMA value, or `None` for cold start (returns `new_value`).
/// - `new_value`: latest observed latency (ms).
/// - `alpha`: smoothing factor in (0, 1]. Higher = more responsive.
pub fn update_ewma(current: Option<f64>, new_value: f64, alpha: f64) -> f64 {
    match current {
        None => new_value,
        Some(c) => alpha * new_value + (1.0 - alpha) * c,
    }
}
