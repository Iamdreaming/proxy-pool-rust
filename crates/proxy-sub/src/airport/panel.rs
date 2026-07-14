//! Airport panel probing and registerability checks.
//!
//! Before registering on an airport site we probe it to determine the panel
//! software (v2board or sspanel) and the registration requirements (email
//! verification, invite codes, recaptcha, email whitelist). [`probe_panel`]
//! performs that detection; [`is_registerable`] decides whether the site can
//! be registered without manual intervention.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Detected airport panel type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelType {
    /// v2board panel (the common modern airport software).
    V2Board,
    /// sspanel (the older ss-panel derived software).
    SsPanel,
    /// Panel type could not be determined.
    Unknown,
}

/// Registration requirements detected for an airport site.
#[derive(Debug, Clone)]
pub struct RegisterRequirement {
    /// Whether account registration requires email verification.
    pub email_verify: bool,
    /// Whether registration requires an invite code.
    pub invite_required: bool,
    /// Whether registration is protected by a recaptcha challenge.
    pub recaptcha: bool,
    /// Email domain whitelist suffixes accepted for registration.
    pub email_whitelist: Vec<String>,
    /// Detected panel type.
    pub panel_type: PanelType,
}

/// Probe a domain for its panel type and registration requirements.
///
/// Tries the v2board guest config endpoint (and an alternate scheme), then
/// falls back to sspanel login-endpoint detection. Returns `None` only when
/// no panel could be identified at all.
pub async fn probe_panel(domain: &str, client: &reqwest::Client) -> Option<RegisterRequirement> {
    // 1. v2board guest/comm/config endpoint.
    let cfg_url = format!("https://{domain}/api/v1/guest/comm/config");
    if let Some(req) = fetch_v2board_config(&cfg_url, client).await {
        return Some(req);
    }

    // 2. Alternate v2board scheme.
    let alt_url = format!("https://{domain}/api?scheme=guest/comm/config");
    if let Some(req) = fetch_v2board_config(&alt_url, client).await {
        return Some(req);
    }

    // 3. sspanel detection via login endpoints.
    let login_url = format!("https://{domain}/auth/login");
    let passport_url = format!("https://{domain}/api/v1/passport/auth/login");
    let login_ok = status_is_200(client.get(&login_url)).await;
    let passport_ok = status_is_200(client.get(&passport_url)).await;

    if passport_ok {
        // A working passport endpoint implies a v2board panel.
        return Some(default_requirement(PanelType::V2Board));
    }
    if login_ok {
        // Login page but no passport endpoint implies sspanel.
        return Some(default_requirement(PanelType::SsPanel));
    }

    tracing::warn!(domain = %domain, "airport panel probe failed");
    None
}

/// Whether a site is registerable without manual intervention.
///
/// A site is NOT registerable if it requires an invite code or a recaptcha
/// challenge, or if it whitelists email domains and none of them permit a
/// common free provider (gmail). All other sites are considered registerable.
pub fn is_registerable(req: &RegisterRequirement) -> bool {
    if req.invite_required || req.recaptcha {
        return false;
    }
    if !req.email_whitelist.is_empty()
        && req.email_verify
        && !req.email_whitelist.iter().any(|d| d.contains("gmail"))
    {
        return false;
    }
    true
}

/// Build a [`RegisterRequirement`] with all flags defaulting to `false`.
fn default_requirement(panel_type: PanelType) -> RegisterRequirement {
    RegisterRequirement {
        email_verify: false,
        invite_required: false,
        recaptcha: false,
        email_whitelist: Vec::new(),
        panel_type,
    }
}

/// Fetch and parse a v2board guest config endpoint into requirements.
async fn fetch_v2board_config(url: &str, client: &reqwest::Client) -> Option<RegisterRequirement> {
    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(url = %url, "airport config request failed: {e}");
            return None;
        }
    };
    if !resp.status().is_success() {
        return None;
    }
    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(url = %url, "airport config parse failed: {e}");
            return None;
        }
    };

    let email_verify = body
        .get("is_email_verify")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let invite_required = body
        .get("is_invite_force")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let recaptcha = body
        .get("is_recaptcha")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let email_whitelist: Vec<String> = body
        .get("email_whitelist_suffix")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|e| e.as_str().map(String::from)).collect())
        .unwrap_or_default();

    Some(RegisterRequirement {
        email_verify,
        invite_required,
        recaptcha,
        email_whitelist,
        panel_type: PanelType::V2Board,
    })
}

/// Return `true` if the request resolves to an HTTP 200, `false` otherwise.
async fn status_is_200(req: reqwest::RequestBuilder) -> bool {
    match req.send().await {
        Ok(r) => r.status() == reqwest::StatusCode::OK,
        Err(_) => false,
    }
}
