//! Auto check-in and traffic renewal for registered airport sites.
//!
//! Many airport (v2board/sspanel) panels expose a daily `/user/checkin`
//! endpoint that grants bonus traffic, and a free-plan `order` endpoint that
//! re-provisions traffic when a subscription is exhausted or about to expire.
//!
//! This module implements:
//! - [`checkin`] — POST the panel check-in endpoint with the stored auth token.
//! - [`renew_if_needed`] — re-order the free plan when traffic or time is low.
//! - Redis persistence of the last check-in result ([`save_checkin_result`],
//!   [`load_checkin_status`], [`load_checkin_statuses`]).

use crate::airport::AirportAccount;
use crate::ops::SubscriptionMeta;
use anyhow::Result;
use chrono::{DateTime, Utc};
use proxy_core::store::ProxyStore;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

/// Result of a single check-in (or renewal) attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckinResult {
    /// Airport domain the check-in ran against.
    pub domain: String,
    /// Whether the panel reported success.
    pub success: bool,
    /// Human-readable message from the panel or an error description.
    pub message: String,
    /// When the check-in attempt completed.
    pub checked_in_at: DateTime<Utc>,
}

/// Last persisted check-in status for an airport, read back from Redis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckinStatus {
    /// Airport domain.
    pub domain: String,
    /// When the last check-in ran (absent if never recorded).
    #[serde(default)]
    pub last_checkin_at: Option<DateTime<Utc>>,
    /// Whether the last check-in succeeded.
    #[serde(default)]
    pub success: bool,
    /// Last message from the panel or an error description.
    #[serde(default)]
    pub message: String,
}

/// Perform check-in for an airport site.
///
/// POSTs `https://{domain}/user/checkin` with `Authorization: Bearer {token}`
/// and parses the panel JSON response for a success flag and message. Network
/// or parse failures are captured in the returned [`CheckinResult`] rather than
/// propagated, so a single failing site never aborts a batch.
pub async fn checkin(domain: &str, token: &str, client: &reqwest::Client) -> CheckinResult {
    let url = format!("https://{domain}/user/checkin");
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await;

    match response {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let success = status.is_success();
            let message = parse_checkin_message(&body).unwrap_or_else(|| {
                if success {
                    "ok".to_string()
                } else {
                    format!("http {status}")
                }
            });
            CheckinResult {
                domain: domain.to_string(),
                success,
                message,
                checked_in_at: Utc::now(),
            }
        }
        Err(e) => CheckinResult {
            domain: domain.to_string(),
            success: false,
            message: format!("request failed: {e}"),
            checked_in_at: Utc::now(),
        },
    }
}

/// Check subscription metadata and trigger renewal if needed.
///
/// Renewal is triggered when:
/// - `remaining_ratio >= 0.8` (traffic nearly exhausted), OR
/// - `remaining_days <= 5` (expiring soon)
///
/// Renewal re-orders the free plan via the airport API
/// ([`order_free_plan`]). Returns `None` when no metadata is supplied or the
/// subscription is still healthy (no renewal required).
pub async fn renew_if_needed(
    account: &AirportAccount,
    meta: Option<&SubscriptionMeta>,
    client: &reqwest::Client,
) -> Option<CheckinResult> {
    let needs_renewal = match meta {
        Some(m) => m.remaining_ratio >= 0.8 || m.remaining_days.is_some_and(|d| d <= 5.0),
        None => return None,
    };
    if !needs_renewal {
        return None;
    }

    let token = account.token.as_deref()?;
    let message = match order_free_plan(&account.domain, token, client).await {
        Ok(msg) => {
            tracing::info!(domain = %account.domain, "airport free plan renewed");
            msg
        }
        Err(e) => {
            tracing::warn!(domain = %account.domain, error = %e, "airport renewal failed");
            format!("renewal failed: {e}")
        }
    };
    Some(CheckinResult {
        domain: account.domain.clone(),
        success: !message.starts_with("renewal failed"),
        message,
        checked_in_at: Utc::now(),
    })
}

/// Best-effort: order a free plan for an already-registered account.
///
/// Fetches available plans and, if a free (price 0 or name containing a free
/// marker) plan exists, submits an order. Returns the panel message on success.
async fn order_free_plan(domain: &str, token: &str, client: &reqwest::Client) -> Result<String> {
    let fetch_url = format!("https://{domain}/api/v1/user/server/fetch");
    let resp = client
        .get(&fetch_url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("free plan fetch returned {}", resp.status());
    }
    let v: serde_json::Value = resp.json().await?;
    let id = crate::airport::panel::find_free_plan_id(&v)
        .ok_or_else(|| anyhow::anyhow!("no free plan found"))?;
    let order_url = format!("https://{domain}/api/v1/user/order/save");
    let resp = client
        .post(&order_url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "plan_id": id }))
        .send()
        .await?;
    if resp.status().is_success() {
        Ok("free plan ordered".into())
    } else {
        anyhow::bail!("order request returned {}", resp.status())
    }
}

/// Persist a check-in result to Redis.
///
/// Key: `airport:checkin:{domain}` — hash with `last_checkin_at`, `success`,
/// and `message`.
pub async fn save_checkin_result(store: &ProxyStore, result: &CheckinResult) -> Result<()> {
    let mut conn = store.raw_conn();
    let key = format!("airport:checkin:{}", result.domain);
    let _: () = conn
        .hset(&key, "last_checkin_at", result.checked_in_at.to_rfc3339())
        .await?;
    let _: () = conn
        .hset(&key, "success", result.success.to_string())
        .await?;
    let _: () = conn.hset(&key, "message", &result.message).await?;
    Ok(())
}

/// Load the last check-in status for a single airport domain.
///
/// Returns `None` when no check-in record exists for the domain.
pub async fn load_checkin_status(store: &ProxyStore, domain: &str) -> Option<CheckinStatus> {
    let mut conn = store.raw_conn();
    let key = format!("airport:checkin:{domain}");
    let map: std::collections::HashMap<String, String> = match conn.hgetall(&key).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(domain = %domain, "failed to load checkin status: {e}");
            return None;
        }
    };
    if map.is_empty() {
        return None;
    }
    let last_checkin_at = map
        .get("last_checkin_at")
        .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
        .map(|dt| dt.with_timezone(&Utc));
    let success = map
        .get("success")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);
    let message = map.get("message").cloned().unwrap_or_default();
    Some(CheckinStatus {
        domain: domain.to_string(),
        last_checkin_at,
        success,
        message,
    })
}

/// Load the last check-in status for all registered airport accounts.
///
/// Accounts without a recorded check-in are omitted from the result.
pub async fn load_checkin_statuses(store: &ProxyStore) -> Vec<CheckinStatus> {
    let mut conn = store.raw_conn();
    let members: Vec<String> = match conn.smembers("airport:accounts").await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("failed to load airport account set: {e}");
            return Vec::new();
        }
    };
    let mut out = Vec::with_capacity(members.len());
    for domain in &members {
        if let Some(status) = load_checkin_status(store, domain).await {
            out.push(status);
        }
    }
    out
}

/// Extract a human-readable message from a panel check-in JSON response.
///
/// Tries `msg`, then `message`, then a nested `data.checkin` string. Returns
/// `None` if the body is not JSON or carries none of these fields.
fn parse_checkin_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("msg")
        .and_then(|m| m.as_str())
        .map(String::from)
        .or_else(|| v.get("message").and_then(|m| m.as_str()).map(String::from))
        .or_else(|| {
            v.get("data")
                .and_then(|d| d.get("checkin"))
                .and_then(|c| c.as_str())
                .map(String::from)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_checkin_message_msg_field() {
        let body = r#"{"ret":1,"msg":"Check-in successful","data":{}}"#;
        assert_eq!(
            parse_checkin_message(body).as_deref(),
            Some("Check-in successful")
        );
    }

    #[test]
    fn test_parse_checkin_message_message_field() {
        let body = r#"{"status":"success","message":"checked in"}"#;
        assert_eq!(parse_checkin_message(body).as_deref(), Some("checked in"));
    }

    #[test]
    fn test_parse_checkin_message_data_checkin_field() {
        let body = r#"{"data":{"checkin":"done"}}"#;
        assert_eq!(parse_checkin_message(body).as_deref(), Some("done"));
    }

    #[test]
    fn test_parse_checkin_message_none_for_garbage() {
        assert_eq!(parse_checkin_message("not json at all"), None);
        assert_eq!(parse_checkin_message(r#"{"foo":1}"#), None);
    }

    #[test]
    fn test_checkin_result_serialization_roundtrip() {
        let result = CheckinResult {
            domain: "example.com".into(),
            success: true,
            message: "ok".into(),
            checked_in_at: Utc::now(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: CheckinResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.domain, result.domain);
        assert!(back.success);
        assert_eq!(back.message, result.message);
    }

    #[test]
    fn test_checkin_status_serialization() {
        let status = CheckinStatus {
            domain: "example.com".into(),
            last_checkin_at: Some(Utc::now()),
            success: false,
            message: "http 500".into(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"success\":false"));
        assert!(json.contains("\"domain\":\"example.com\""));
    }

    #[test]
    fn test_renew_if_needed_returns_none_without_meta() {
        let account = AirportAccount {
            domain: "example.com".into(),
            email: "a@b.com".into(),
            password: "pw".into(),
            token: Some("tok".into()),
            sub_url: None,
            panel_type: crate::airport::PanelType::Unknown,
            registered_at: Utc::now(),
        };
        // No client needed: None meta short-circuits before any network call.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(renew_if_needed(&account, None, &reqwest::Client::new()));
        assert!(result.is_none());
    }

    #[test]
    fn test_renew_if_needed_triggers_when_low_traffic() {
        let account = AirportAccount {
            domain: "example.com".into(),
            email: "a@b.com".into(),
            password: "pw".into(),
            token: Some("tok".into()),
            sub_url: None,
            panel_type: crate::airport::PanelType::Unknown,
            registered_at: Utc::now(),
        };
        let meta = SubscriptionMeta {
            upload: 0,
            download: 0,
            total: 100,
            expire: None,
            remaining_ratio: 0.9,
            remaining_days: None,
            health: 0.9,
        };
        // Meta is unhealthy → returns Some (network attempt may fail, but the
        // decision to renew is made). We only assert it is not None.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(renew_if_needed(
            &account,
            Some(&meta),
            &reqwest::Client::new(),
        ));
        assert!(result.is_some());
    }

    #[test]
    fn test_renew_if_needed_skips_when_healthy() {
        let account = AirportAccount {
            domain: "example.com".into(),
            email: "a@b.com".into(),
            password: "pw".into(),
            token: Some("tok".into()),
            sub_url: None,
            panel_type: crate::airport::PanelType::Unknown,
            registered_at: Utc::now(),
        };
        let meta = SubscriptionMeta {
            upload: 0,
            download: 10,
            total: 100,
            expire: None,
            remaining_ratio: 0.9, // would trigger...
            remaining_days: Some(30.0),
            health: 0.9,
        };
        // remaining_ratio >= 0.8 triggers, so this meta IS unhealthy. Build a
        // clearly healthy meta instead.
        let healthy = SubscriptionMeta {
            upload: 0,
            download: 10,
            total: 100,
            expire: None,
            remaining_ratio: 0.5,
            remaining_days: Some(30.0),
            health: 0.5,
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let triggered = rt.block_on(renew_if_needed(
            &account,
            Some(&meta),
            &reqwest::Client::new(),
        ));
        let skipped = rt.block_on(renew_if_needed(
            &account,
            Some(&healthy),
            &reqwest::Client::new(),
        ));
        assert!(triggered.is_some());
        assert!(skipped.is_none());
    }
}
