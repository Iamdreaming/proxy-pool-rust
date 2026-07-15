//! Source origin credibility system.
//!
//! Each subscription source carries a [`SourceOrigin`] tag that determines how
//! long discovered links remain credible before being downgraded or evicted.
//! This works alongside the per-fetcher circuit breaker (short-term, seconds
//! to minutes) to provide long-term lifecycle management (days to weeks).

use serde::{Deserialize, Serialize};

/// Origin of a subscription source, with an associated credibility window.
///
/// The `expiry_days` value controls how aggressively links from this origin
/// are downgraded in the `recommend_apply` gate:
/// - Within the window → normal gate thresholds apply.
/// - Past the window but under 2× → downgraded to `Review`.
/// - Past 2× the window → downgraded to `Reject`.
/// - `Owned` / `Manual` origins never expire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceOrigin {
    /// User-owned subscriptions (manually configured, never expire).
    Owned,
    /// Manually added static URLs (never expire).
    Manual,
    /// Discovered via GitHub search (20-day window).
    GitHub,
    /// Discovered via airport auto-registration (7-day window).
    Airport,
    /// Discovered via aggregator services (10-day window).
    Aggregator,
    /// Discovered via Telegram channels (3-day window).
    Telegram,
    /// Discovered via LLM web-search (grok). Low-trust, 3-day window.
    Search,
}

impl SourceOrigin {
    /// Credibility window in days. `u32::MAX` means "never expires".
    pub fn expiry_days(self) -> u32 {
        match self {
            Self::Owned | Self::Manual => u32::MAX,
            Self::GitHub => 20,
            Self::Airport => 7,
            Self::Aggregator => 10,
            Self::Telegram => 3,
            Self::Search => 3,
        }
    }

    /// Whether this origin never expires.
    pub fn is_permanent(self) -> bool {
        matches!(self, Self::Owned | Self::Manual)
    }

    /// Determine the credibility degradation level based on days since last
    /// successful refresh.
    ///
    /// Returns `None` for permanent origins (no degradation).
    /// Returns `Some(CredibilityLevel)` for time-based origins.
    pub fn degradation(self, days_since_success: u32) -> Option<CredibilityLevel> {
        if self.is_permanent() {
            return None;
        }
        let window = self.expiry_days();
        Some(if days_since_success >= window * 2 {
            CredibilityLevel::Expired
        } else if days_since_success >= window {
            CredibilityLevel::Stale
        } else {
            CredibilityLevel::Fresh
        })
    }
}

impl std::fmt::Display for SourceOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            SourceOrigin::Owned => "owned",
            SourceOrigin::Manual => "manual",
            SourceOrigin::GitHub => "git_hub",
            SourceOrigin::Airport => "airport",
            SourceOrigin::Aggregator => "aggregator",
            SourceOrigin::Telegram => "telegram",
            SourceOrigin::Search => "search",
        };
        f.write_str(s)
    }
}

/// Credibility level derived from time since last successful refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredibilityLevel {
    /// Within the credibility window — normal thresholds.
    Fresh,
    /// Past the window but under 2× — downgrade to Review.
    Stale,
    /// Past 2× the window — downgrade to Reject.
    Expired,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permanent_origins_never_expire() {
        assert!(SourceOrigin::Owned.is_permanent());
        assert!(SourceOrigin::Manual.is_permanent());
        assert!(!SourceOrigin::GitHub.is_permanent());
        assert!(!SourceOrigin::Telegram.is_permanent());
    }

    #[test]
    fn test_expiry_days() {
        assert_eq!(SourceOrigin::Owned.expiry_days(), u32::MAX);
        assert_eq!(SourceOrigin::Manual.expiry_days(), u32::MAX);
        assert_eq!(SourceOrigin::GitHub.expiry_days(), 20);
        assert_eq!(SourceOrigin::Airport.expiry_days(), 7);
        assert_eq!(SourceOrigin::Aggregator.expiry_days(), 10);
        assert_eq!(SourceOrigin::Telegram.expiry_days(), 3);
        assert_eq!(SourceOrigin::Search.expiry_days(), 3);
    }

    #[test]
    fn test_degradation_permanent_always_none() {
        assert_eq!(SourceOrigin::Owned.degradation(0), None);
        assert_eq!(SourceOrigin::Owned.degradation(999), None);
        assert_eq!(SourceOrigin::Manual.degradation(999), None);
    }

    #[test]
    fn test_degradation_telegram() {
        // Telegram: 3-day window
        assert_eq!(
            SourceOrigin::Telegram.degradation(0),
            Some(CredibilityLevel::Fresh)
        );
        assert_eq!(
            SourceOrigin::Telegram.degradation(3),
            Some(CredibilityLevel::Stale)
        );
        assert_eq!(
            SourceOrigin::Telegram.degradation(6),
            Some(CredibilityLevel::Expired)
        );
        assert_eq!(
            SourceOrigin::Telegram.degradation(7),
            Some(CredibilityLevel::Expired)
        );
    }

    #[test]
    fn test_degradation_github() {
        // GitHub: 20-day window
        assert_eq!(
            SourceOrigin::GitHub.degradation(19),
            Some(CredibilityLevel::Fresh)
        );
        assert_eq!(
            SourceOrigin::GitHub.degradation(20),
            Some(CredibilityLevel::Stale)
        );
        assert_eq!(
            SourceOrigin::GitHub.degradation(40),
            Some(CredibilityLevel::Expired)
        );
    }

    #[test]
    fn test_degradation_airport() {
        // Airport: 7-day window
        assert_eq!(
            SourceOrigin::Airport.degradation(6),
            Some(CredibilityLevel::Fresh)
        );
        assert_eq!(
            SourceOrigin::Airport.degradation(7),
            Some(CredibilityLevel::Stale)
        );
        assert_eq!(
            SourceOrigin::Airport.degradation(14),
            Some(CredibilityLevel::Expired)
        );
    }

    #[test]
    fn test_display_roundtrip() {
        let origin = SourceOrigin::Telegram;
        let s = origin.to_string();
        assert_eq!(s, "telegram");
    }

    #[test]
    fn test_display_matches_serde() {
        for origin in [
            SourceOrigin::Owned,
            SourceOrigin::Manual,
            SourceOrigin::GitHub,
            SourceOrigin::Airport,
            SourceOrigin::Aggregator,
            SourceOrigin::Telegram,
            SourceOrigin::Search,
        ] {
            let via_serde = serde_json::to_value(origin)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string();
            assert_eq!(origin.to_string(), via_serde);
        }
    }

    #[test]
    fn test_serde_roundtrip() {
        for origin in [
            SourceOrigin::Owned,
            SourceOrigin::Manual,
            SourceOrigin::GitHub,
            SourceOrigin::Airport,
            SourceOrigin::Aggregator,
            SourceOrigin::Telegram,
            SourceOrigin::Search,
        ] {
            let json = serde_json::to_string(&origin).unwrap();
            let back: SourceOrigin = serde_json::from_str(&json).unwrap();
            assert_eq!(origin, back, "roundtrip failed for {origin:?}");
        }
    }
}
