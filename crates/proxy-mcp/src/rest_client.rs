//! HTTP client for the main proxy-pool REST API.
//!
//! The standalone MCP server holds no in-process pool state; every tool that
//! needs live pool/scheduler/route/xray data calls the main service over REST
//! through this client. Failures map to a structured error so MCP tools return
//! `{"status":"error",...}` instead of hanging or panicking.

use serde_json::Value;
use std::time::Duration;

/// Default request timeout for upstream REST calls.
const REQUEST_TIMEOUT_SECS: u64 = 15;

/// Error talking to the upstream REST API.
#[derive(Debug)]
pub enum RestError {
    /// Transport-level failure (connect, timeout, DNS).
    Transport(String),
    /// Non-2xx HTTP status with the response body (truncated).
    Status { code: u16, body: String },
    /// Response body was not valid JSON.
    Decode(String),
}

impl std::fmt::Display for RestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "upstream transport error: {e}"),
            Self::Status { code, body } => write!(f, "upstream returned HTTP {code}: {body}"),
            Self::Decode(e) => write!(f, "upstream response decode error: {e}"),
        }
    }
}

impl std::error::Error for RestError {}

/// Thin client over the main service REST API.
#[derive(Clone)]
pub struct RestClient {
    base: String,
    http: reqwest::Client,
}

impl RestClient {
    /// Create a client targeting `base` (e.g. `http://proxy-pool:8000`).
    pub fn new(base: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .build()
            .unwrap_or_default();
        Self {
            base: normalize_base(base.into()),
            http,
        }
    }

    /// Base URL this client targets.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// GET `path` with optional query pairs, returning parsed JSON.
    pub async fn get_json(&self, path: &str, query: &[(&str, String)]) -> Result<Value, RestError> {
        let url = build_url(&self.base, path);
        let mut req = self.http.get(&url);
        if !query.is_empty() {
            req = req.query(query);
        }
        Self::send(req).await
    }

    /// POST `path` with an optional JSON body, returning parsed JSON.
    pub async fn post_json(&self, path: &str, body: Option<&Value>) -> Result<Value, RestError> {
        self.post_json_query(path, &[], body).await
    }

    /// POST `path` with query pairs and an optional JSON body.
    pub async fn post_json_query(
        &self,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Value, RestError> {
        let url = build_url(&self.base, path);
        let mut req = self.http.post(&url);
        if !query.is_empty() {
            req = req.query(query);
        }
        if let Some(body) = body {
            req = req.json(body);
        }
        Self::send(req).await
    }

    /// DELETE `path`, returning parsed JSON.
    pub async fn delete_json(&self, path: &str) -> Result<Value, RestError> {
        let url = build_url(&self.base, path);
        Self::send(self.http.delete(&url)).await
    }

    /// PUT `path` with an optional JSON body, returning parsed JSON.
    pub async fn put_json(&self, path: &str, body: Option<&Value>) -> Result<Value, RestError> {
        self.put_json_query(path, &[], body).await
    }

    /// PUT `path` with query pairs and an optional JSON body.
    pub async fn put_json_query(
        &self,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Value, RestError> {
        let url = build_url(&self.base, path);
        let mut req = self.http.put(&url);
        if !query.is_empty() {
            req = req.query(query);
        }
        if let Some(body) = body {
            req = req.json(body);
        }
        Self::send(req).await
    }

    async fn send(req: reqwest::RequestBuilder) -> Result<Value, RestError> {
        let resp = req
            .send()
            .await
            .map_err(|e| RestError::Transport(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| RestError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(RestError::Status {
                code: status.as_u16(),
                body: truncate(&text, 512),
            });
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| RestError::Decode(e.to_string()))
    }
}

/// Strip a single trailing slash so `build_url` joins cleanly.
fn normalize_base(base: String) -> String {
    base.strip_suffix('/').map(str::to_string).unwrap_or(base)
}

/// Join a normalized base and an absolute path (`/api/...`).
fn build_url(base: &str, path: &str) -> String {
    if path.starts_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

/// Truncate a body (by chars, panic-safe) for inclusion in an error message.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{truncated}…")
    }
}

/// Percent-encode a single URL path segment (RFC 3986 unreserved set kept).
///
/// Used for fetcher/subscription ids that may contain `:` etc.
pub fn urlencode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for &b in segment.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_base_strips_trailing_slash() {
        assert_eq!(normalize_base("http://x:8000/".into()), "http://x:8000");
        assert_eq!(normalize_base("http://x:8000".into()), "http://x:8000");
    }

    #[test]
    fn build_url_joins_absolute_path() {
        assert_eq!(
            build_url("http://x:8000", "/api/status"),
            "http://x:8000/api/status"
        );
        assert_eq!(
            build_url("http://x:8000", "api/status"),
            "http://x:8000/api/status"
        );
    }

    #[test]
    fn truncate_caps_length() {
        assert_eq!(truncate("abc", 10), "abc");
        assert_eq!(truncate("abcdef", 3), "abc…");
    }

    #[test]
    fn rest_error_display_is_readable() {
        let e = RestError::Status {
            code: 503,
            body: "down".into(),
        };
        assert!(e.to_string().contains("503"));
        assert!(e.to_string().contains("down"));
    }
}
