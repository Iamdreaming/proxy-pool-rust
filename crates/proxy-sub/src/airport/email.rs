//! Cloudflare Worker temp-email client
//! ([dreamhunter2333/cloudflare_temp_email](https://github.com/dreamhunter2333/cloudflare_temp_email)).
//!
//! This client provides disposable email addresses backed by a Cloudflare
//! Worker + D1/KV store. The airport registration flow uses it to obtain a
//! throwaway address, register on a panel, and poll for the verification code.

use anyhow::Result;
use serde::Deserialize;
use serde_json;
use std::time::Duration;

/// Cloudflare Worker temp-email client (dreamhunter2333/cloudflare_temp_email).
#[derive(Clone)]
pub struct CloudflareEmailClient {
    base_url: String,
    client: reqwest::Client,
    admin_auth: Option<String>,
}

/// A created temporary email address and its auth JWT.
pub struct TempEmail {
    /// The temporary email address (e.g. `user@domain.tld`).
    pub address: String,
    /// Auth JWT used to poll for received mail.
    pub jwt: String,
}

#[derive(Deserialize, Default)]
struct NewAddressResponse {
    #[serde(default)]
    jwt: String,
    #[serde(default)]
    address: String,
}

#[derive(Deserialize, Default)]
struct ParsedMailsResponse {
    #[serde(default)]
    data: Vec<ParsedMail>,
}

#[derive(Deserialize, Default)]
struct ParsedMail {
    #[serde(default)]
    content: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    html: String,
}

impl CloudflareEmailClient {
    /// Create a new client for the given Cloudflare Worker base URL.
    ///
    /// `admin_auth` is the optional site access password sent as the
    /// `x-custom-auth` header on address creation (some workers require it).
    /// The internal HTTP client times out after 30s.
    pub fn new(base_url: String, admin_auth: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("proxy-pool-rust")
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("failed to build cloudflare email client: {e}");
                reqwest::Client::new()
            });
        Self {
            base_url,
            client,
            admin_auth,
        }
    }

    /// Create a new temporary email address, optionally scoped to `domain`.
    pub async fn create_temp_email(&self, domain: &str) -> Result<TempEmail> {
        let url = format!("{}/api/new_address", self.base_url);
        let mut req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "domain": domain }));
        if let Some(auth) = &self.admin_auth {
            req = req.header("x-custom-auth", auth);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("create_temp_email http {}", resp.status());
        }
        let body: NewAddressResponse = resp.json().await?;
        if body.jwt.is_empty() || body.address.is_empty() {
            anyhow::bail!("create_temp_email response missing jwt/address");
        }
        Ok(TempEmail {
            address: body.address,
            jwt: body.jwt,
        })
    }

    /// Poll for a verification code in received mail until `timeout_secs` elapse.
    ///
    /// Sleeps `interval_secs` between polls. The first mail containing a
    /// verification code (extracted by [`Self::extract_verification_code`]) wins.
    /// Returns an error if no code is found within the timeout.
    pub async fn poll_for_code(
        &self,
        jwt: &str,
        timeout_secs: u64,
        interval_secs: u64,
    ) -> Result<String> {
        let url = format!("{}/api/parsed_mails", self.base_url);
        let start = std::time::Instant::now();
        let interval = Duration::from_secs(interval_secs.max(1));
        loop {
            let parsed: Vec<ParsedMail> = {
                let resp = self
                    .client
                    .get(&url)
                    .header("Authorization", format!("Bearer {jwt}"))
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    Vec::new()
                } else {
                    let text = resp.text().await.unwrap_or_default();
                    if let Ok(r) = serde_json::from_str::<ParsedMailsResponse>(&text) {
                        r.data
                    } else {
                        serde_json::from_str::<Vec<ParsedMail>>(&text).unwrap_or_default()
                    }
                }
            };
            for mail in &parsed {
                let blob = format!(
                    "{}\n{}\n{}\n{}",
                    mail.content, mail.text, mail.body, mail.html
                );
                if let Some(code) = Self::extract_verification_code(&blob) {
                    return Ok(code);
                }
            }
            if start.elapsed().as_secs() >= timeout_secs {
                break;
            }
            tokio::time::sleep(interval).await;
        }
        anyhow::bail!("verification code not found")
    }

    /// Scan a mail body for a verification code.
    ///
    /// Returns the first run of exactly 6 ASCII digits. If no 6-digit run is
    /// found, falls back to the first run of 4–8 digits.
    pub fn extract_verification_code(body: &str) -> Option<String> {
        let mut best: Option<String> = None;
        let mut run: Vec<char> = Vec::new();
        let flush = |run: &mut Vec<char>, best: &mut Option<String>| {
            let len = run.len();
            if len == 6 || ((4..=8).contains(&len) && best.is_none()) {
                *best = Some(run.iter().collect());
            }
            run.clear();
        };
        for c in body.chars() {
            if c.is_ascii_digit() {
                run.push(c);
            } else {
                flush(&mut run, &mut best);
                if best.as_ref().map(|s| s.len() == 6).unwrap_or(false) {
                    return best;
                }
            }
        }
        flush(&mut run, &mut best);
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_verification_code_six_digit() {
        let body = "Your verification code is 123456 please use it soon";
        assert_eq!(
            CloudflareEmailClient::extract_verification_code(body),
            Some("123456".to_string())
        );
    }

    #[test]
    fn test_extract_verification_code_no_digits() {
        let body = "there are no digits in this message at all";
        assert_eq!(CloudflareEmailClient::extract_verification_code(body), None);
    }

    #[test]
    fn test_extract_verification_code_fallback_four_to_eight() {
        // No 6-digit run, but a 5-digit run should be accepted as fallback.
        let body = "code: 12345 done";
        assert_eq!(
            CloudflareEmailClient::extract_verification_code(body),
            Some("12345".to_string())
        );
    }
}
