//! Airport account registration on v2board/sspanel panels.
//!
//! [`AirportRegistrar`] ties together the temp-email client and the panel
//! HTTP APIs to register a free account on a discovered airport site, poll for
//! any email verification, and extract the resulting subscription URL. All
//! network steps are best-effort: a partially successful registration still
//! returns an [`AirportAccount`] so the attempt can be persisted.

use super::email::CloudflareEmailClient;
use super::panel::{PanelType, RegisterRequirement};
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};

/// Persisted airport account info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirportAccount {
    /// Airport domain the account was registered on.
    pub domain: String,
    /// Registered email address.
    pub email: String,
    /// Registered password.
    pub password: String,
    /// Auth token returned by the panel, if any.
    pub token: Option<String>,
    /// Subscription URL, if one could be obtained.
    pub sub_url: Option<String>,
    /// Detected panel type.
    pub panel_type: PanelType,
    /// When the account was registered.
    pub registered_at: DateTime<Utc>,
}

/// Registers free accounts on airport (v2board/sspanel) panels.
#[derive(Clone)]
pub struct AirportRegistrar {
    client: reqwest::Client,
    email_client: CloudflareEmailClient,
    email_domain: String,
}

impl AirportRegistrar {
    /// Create a new registrar backed by a Cloudflare temp-email worker.
    ///
    /// `email_domain` is the domain requested from the temp-email worker (a
    /// domain the worker owns); an empty string lets the worker pick its default.
    pub fn new(
        cloudflare_worker_url: String,
        cloudflare_admin_auth: Option<String>,
        email_domain: String,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("proxy-pool-rust")
            .build()
            .unwrap_or_default();
        let email_client = CloudflareEmailClient::new(cloudflare_worker_url, cloudflare_admin_auth);
        Self {
            client,
            email_client,
            email_domain,
        }
    }

    /// Register a free account on the given airport domain.
    ///
    /// Creates a temp email, registers on the panel, and best-effort verifies
    /// the email and fetches a subscription URL. Returns an [`AirportAccount`]
    /// even when some optional step fails, so the attempt is still recorded.
    pub async fn register_airport(
        &self,
        domain: &str,
        req: &RegisterRequirement,
    ) -> anyhow::Result<AirportAccount> {
        let temp = self
            .email_client
            .create_temp_email(&self.email_domain)
            .await?;
        let pw = generate_password(16);

        let reg_url = format!("https://{domain}/api/v1/passport/auth/register");
        let mut body = serde_json::json!({
            "email": temp.address,
            "password": pw,
            "password_confirmation": pw,
        });
        if !req.invite_required {
            body["invite_code"] = serde_json::Value::String(String::new());
        }

        let token: Option<String> = {
            let resp = self.client.post(&reg_url).json(&body).send().await?;
            if resp.status().is_success() {
                match resp.json::<serde_json::Value>().await {
                    Ok(v) => v
                        .get("data")
                        .and_then(|d| {
                            d.get("token")
                                .or_else(|| d.get("auth_data").and_then(|a| a.get("token")))
                        })
                        .and_then(|t| t.as_str())
                        .map(String::from),
                    Err(e) => {
                        tracing::warn!(domain = %domain, "airport register parse failed: {e}");
                        None
                    }
                }
            } else {
                None
            }
        };

        // Best-effort email verification.
        if req.email_verify {
            match self.email_client.poll_for_code(&temp.jwt, 120, 5).await {
                Ok(code) => {
                    let verify_url = format!("https://{domain}/api/v1/passport/auth/verify");
                    if let Err(e) = self
                        .client
                        .post(&verify_url)
                        .json(&serde_json::json!({ "email": temp.address, "code": code }))
                        .send()
                        .await
                    {
                        tracing::warn!(domain = %domain, "airport email verify request failed: {e}");
                    }
                }
                Err(e) => {
                    tracing::warn!(domain = %domain, "airport email verification failed: {e}");
                }
            }
        }

        // Best-effort: order a free plan so the subscription URL is active.
        if let Some(t) = &token
            && let Err(e) = self.order_free_plan(domain, t).await
        {
            tracing::warn!(domain = %domain, "airport order free plan failed: {e}");
        }

        // Best-effort subscription URL.
        let sub_url = self
            .get_subscribe_url(domain, token.as_deref().unwrap_or(""))
            .await
            .ok();

        Ok(AirportAccount {
            domain: domain.to_string(),
            email: temp.address,
            password: pw,
            token,
            sub_url,
            panel_type: req.panel_type.clone(),
            registered_at: Utc::now(),
        })
    }

    /// Best-effort: order a free plan for an already-registered account.
    ///
    /// Fetches the available plans and, if a free (price 0 or name containing
    /// a free marker) plan exists, submits an order. Returns `Ok(())` even when
    /// no free plan is found, so registration still succeeds.
    pub async fn order_free_plan(&self, domain: &str, token: &str) -> anyhow::Result<()> {
        let fetch_url = format!("https://{domain}/api/v1/user/server/fetch");
        let resp = self
            .client
            .get(&fetch_url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?;
        if !resp.status().is_success() {
            tracing::warn!(
                domain = %domain,
                status = %resp.status(),
                "airport free plan fetch returned non-success"
            );
            return Ok(());
        }
        let v: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(domain = %domain, "airport free plan parse failed: {e}");
                return Ok(());
            }
        };
        match crate::airport::panel::find_free_plan_id(&v) {
            Some(id) => {
                let order_url = format!("https://{domain}/api/v1/user/order/save");
                if let Err(e) = self
                    .client
                    .post(&order_url)
                    .header("Authorization", format!("Bearer {token}"))
                    .json(&serde_json::json!({ "plan_id": id }))
                    .send()
                    .await
                {
                    tracing::warn!(domain = %domain, "airport order request failed: {e}");
                }
            }
            None => {
                tracing::warn!(domain = %domain, "airport no free plan found");
            }
        }
        Ok(())
    }

    /// Best-effort: obtain the subscription URL for a registered account.
    ///
    /// Tries the subscribe endpoint, then the user-info endpoint, looking for
    /// a `subscribe_url` / `url` / `sub_url` field. Returns an error if neither
    /// yields a URL.
    pub async fn get_subscribe_url(&self, domain: &str, token: &str) -> anyhow::Result<String> {
        let sub_url = format!("https://{domain}/api/v1/client/subscribe?token={token}");
        let resp = self
            .client
            .get(&sub_url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?;
        if resp.status().is_success()
            && let Ok(v) = resp.json::<serde_json::Value>().await
            && let Some(u) = extract_sub_url(&v)
        {
            return Ok(u);
        }

        let info_url = format!("https://{domain}/api/v1/user/info");
        let resp = self
            .client
            .get(&info_url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?;
        if resp.status().is_success()
            && let Ok(v) = resp.json::<serde_json::Value>().await
            && let Some(u) = extract_sub_url(&v)
        {
            return Ok(u);
        }

        anyhow::bail!("airport subscribe url not found")
    }
}

/// Extract a subscription URL from a panel JSON response.
fn extract_sub_url(v: &serde_json::Value) -> Option<String> {
    v.get("data")
        .and_then(|d| {
            d.get("subscribe_url")
                .or_else(|| d.get("url"))
                .or_else(|| d.get("sub_url"))
        })
        .and_then(|u| u.as_str())
        .map(String::from)
}

/// Generate a random alphanumeric password of the given length.
fn generate_password(len: usize) -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..len)
        .map(|_| {
            let idx = rng.random_range(0..CHARS.len());
            CHARS[idx] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registrar_new_does_not_panic() {
        let _ = AirportRegistrar::new(
            "https://mail.example.com".into(),
            Some("admin-token".into()),
            String::new(),
        );
    }

    #[test]
    fn test_generate_password_length_and_charset() {
        let pw = generate_password(16);
        assert_eq!(pw.len(), 16);
        assert!(pw.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
