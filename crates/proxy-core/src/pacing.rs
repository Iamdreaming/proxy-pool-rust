//! Connection rate pacer: bounds the rate of new outbound connection attempts.

use std::time::Instant;
use tokio::sync::Mutex;

/// Bounds the *rate* of new outbound connection attempts, orthogonal to the
/// concurrency semaphore.
///
/// The validator's semaphore caps how many validations run *simultaneously*,
/// but a burst of fast-failing dead proxies releases slots instantly — so the
/// connection-attempt *rate* can still spike and exhaust resources.
///
/// `rate_per_sec <= 0` disables pacing (acquire is a no-op).
pub struct ConnectionPacer {
    interval: f64,
    state: Mutex<PacerState>,
}

struct PacerState {
    next_at: Option<Instant>,
}

impl ConnectionPacer {
    pub fn new(rate_per_sec: f64) -> Self {
        let interval = if rate_per_sec > 0.0 {
            1.0 / rate_per_sec
        } else {
            0.0
        };
        Self {
            interval,
            state: Mutex::new(PacerState { next_at: None }),
        }
    }

    /// Acquire a slot, waiting if necessary to respect the rate limit.
    pub async fn acquire(&self) {
        if self.interval <= 0.0 {
            return;
        }
        let mut state = self.state.lock().await;
        let now = Instant::now();
        if let Some(next_at) = state.next_at
            && now < next_at
        {
            tokio::time::sleep(next_at - now).await;
        }
        let now = Instant::now();
        state.next_at = Some(
            state.next_at.map_or(now, |na| na.max(now))
                + std::time::Duration::from_secs_f64(self.interval),
        );
    }
}
