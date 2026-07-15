//! LLM web-search discoverer: extracts subscription links from grok search.
//!
//! This discoverer talks to a grok search service over the MCP
//! streamable-HTTP transport using a hand-rolled, minimal JSON-RPC client
//! (no `rmcp` client dependency). The protocol is stateful and must be
//! driven in order:
//!
//! 1. `initialize` — negotiates the protocol and yields an `mcp-session-id`
//!    response header that must be echoed on all subsequent requests.
//! 2. `notifications/initialized` — best-effort notification (no id).
//! 3. `tools/call` — invokes the `web_search` tool once per query; the
//!    free-text result is scanned with [`crate::discover::extract`].
//!
//! Responses use SSE framing (`event: message` / `data: {json}` lines) that
//! this module parses out. All network and parsing errors are logged with
//! `tracing::warn` and skipped — the discoverer never panics and only
//! returns successfully extracted URLs, per the [`Discover`] contract.

use std::collections::HashSet;

use crate::discover::Discover;
use crate::discover::extract::extract_subscription_urls;

/// Built-in default search queries, used when no queries are configured.
const DEFAULT_QUERIES: &[&str] = &[
    "最新免费机场 v2board 订阅链接 clash",
    "free v2ray subscription link github raw 2026",
    "公开 clash 订阅 raw.githubusercontent 最新",
];

/// Configuration for [`SearchDiscover`].
pub struct SearchConfig {
    /// MCP streamable-HTTP endpoint URL of the grok search service.
    pub mcp_url: String,
    /// Bearer auth token. If empty, the `SEARCH_MCP_TOKEN` env var is used.
    pub auth_token: String,
    /// Name of the MCP `web_search` tool. If empty, [`DEFAULT_TOOL_NAME`] is used.
    pub tool_name: String,
    /// Search queries to run. If empty, [`DEFAULT_QUERIES`] are used.
    pub queries: Vec<String>,
    /// Maximum number of queries to run per discovery cycle.
    pub max_queries: usize,
    /// HTTP request timeout in seconds.
    pub timeout_sec: u64,
}

/// Default MCP tool name for the grok search service. The server namespaces the
/// tool as `grok-search-web_search` (verified live); a bare `web_search` yields
/// "Server not found".
const DEFAULT_TOOL_NAME: &str = "grok-search-web_search";

/// A discoverer that runs LLM web searches (grok) and extracts subscription
/// links from the free-text results.
pub struct SearchDiscover {
    config: SearchConfig,
    client: reqwest::Client,
}

impl SearchDiscover {
    /// Create a new search discoverer with the given configuration.
    pub fn new(config: SearchConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_sec))
            .user_agent("proxy-pool-rust")
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("failed to build reqwest client for search: {e}");
                reqwest::Client::new()
            });
        Self { config, client }
    }

    /// Resolve the auth token: prefer the configured value, else the
    /// `SEARCH_MCP_TOKEN` env var. Returns `None` if neither is set.
    fn resolve_token(&self) -> Option<String> {
        if !self.config.auth_token.is_empty() {
            return Some(self.config.auth_token.clone());
        }
        match std::env::var("SEARCH_MCP_TOKEN") {
            Ok(v) if !v.is_empty() => Some(v),
            _ => None,
        }
    }

    /// Resolve the effective query list, truncated to `max_queries`.
    fn resolve_queries(&self) -> Vec<String> {
        let mut queries: Vec<String> = if self.config.queries.is_empty() {
            DEFAULT_QUERIES.iter().map(|s| s.to_string()).collect()
        } else {
            self.config.queries.clone()
        };
        let limit = self.config.max_queries.max(1);
        queries.truncate(limit);
        queries
    }

    /// Build a request to the MCP streamable-HTTP endpoint with standard headers.
    ///
    /// Automatically sets Authorization (Bearer token), Content-Type (application/json),
    /// and Accept (application/json, text/event-stream) headers. If `session_id` is
    /// non-empty, also sets the Mcp-Session-Id header.
    fn mcp_request(&self, token: &str, session_id: &str) -> reqwest::RequestBuilder {
        let mut req = self
            .client
            .post(&self.config.mcp_url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");

        if !session_id.is_empty() {
            req = req.header("Mcp-Session-Id", session_id);
        }

        req
    }

    /// Perform the `initialize` handshake, returning the `mcp-session-id`.
    ///
    /// Returns `None` (after logging) on any network error, non-success
    /// status, missing session id, or unreadable body.
    async fn initialize(&self, token: &str) -> Option<String> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "proxy-pool-rust", "version": "0.1.0" }
            }
        });

        let resp = match self.mcp_request(token, "").json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(name = self.name(), "initialize request failed: {e}");
                return None;
            }
        };

        if !resp.status().is_success() {
            tracing::warn!(
                name = self.name(),
                status = %resp.status(),
                "initialize returned non-success status"
            );
            return None;
        }

        let session_id = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // Drain the body to keep the connection reusable; parse best-effort.
        let _ = resp.text().await;

        match session_id {
            Some(id) if !id.is_empty() => Some(id),
            _ => {
                tracing::warn!(
                    name = self.name(),
                    "initialize response missing mcp-session-id"
                );
                None
            }
        }
    }

    /// Send the `notifications/initialized` notification (best-effort).
    async fn notify_initialized(&self, token: &str, session_id: &str) {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });

        if let Err(e) = self.mcp_request(token, session_id).json(&body).send().await {
            tracing::warn!(name = self.name(), "notifications/initialized failed: {e}");
        }
    }

    /// Invoke the `web_search` tool for a single query and return the result
    /// free-text, or `None` (after logging) on any error.
    async fn call_web_search(
        &self,
        token: &str,
        session_id: &str,
        id: i64,
        query: &str,
    ) -> Option<String> {
        let tool_name = if self.config.tool_name.trim().is_empty() {
            DEFAULT_TOOL_NAME
        } else {
            self.config.tool_name.trim()
        };
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": { "query": query }
            }
        });

        let resp = match self.mcp_request(token, session_id).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(name = self.name(), %query, "tools/call request failed: {e}");
                return None;
            }
        };

        if !resp.status().is_success() {
            tracing::warn!(
                name = self.name(),
                %query,
                status = %resp.status(),
                "tools/call returned non-success status"
            );
            return None;
        }

        let text = match resp.text().await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(name = self.name(), %query, "tools/call read body failed: {e}");
                return None;
            }
        };

        let value = match parse_sse_data(&text) {
            Some(v) => v,
            None => {
                tracing::warn!(name = self.name(), %query, "tools/call response had no parsable data");
                return None;
            }
        };

        if let Some(err) = value.get("error") {
            tracing::warn!(name = self.name(), %query, "tools/call returned error: {err}");
            return None;
        }

        let result = match value.get("result") {
            Some(r) => r,
            None => {
                tracing::warn!(name = self.name(), %query, "tools/call response missing result");
                return None;
            }
        };

        // Concatenate text from every content item (MCP content is an array),
        // plus any top-level `result.text`, so links in later blocks are not lost.
        let mut text = String::new();
        if let Some(items) = result.get("content").and_then(|c| c.as_array()) {
            for item in items {
                if let Some(s) = item.get("text").and_then(|t| t.as_str()) {
                    text.push_str(s);
                    text.push('\n');
                }
            }
        }
        if let Some(s) = result.get("text").and_then(|t| t.as_str()) {
            text.push_str(s);
            text.push('\n');
        }

        if text.trim().is_empty() {
            tracing::warn!(name = self.name(), %query, "tools/call result had no text content");
            return None;
        }
        Some(text)
    }
}

#[async_trait::async_trait]
impl Discover for SearchDiscover {
    fn name(&self) -> &str {
        "search"
    }

    async fn discover(&self) -> Vec<String> {
        let token = match self.resolve_token() {
            Some(t) => t,
            None => {
                tracing::warn!(
                    name = self.name(),
                    "no auth token (config or SEARCH_MCP_TOKEN)"
                );
                return Vec::new();
            }
        };

        if self.config.mcp_url.is_empty() {
            tracing::warn!(name = self.name(), "mcp_url is empty; skipping");
            return Vec::new();
        }

        let queries = self.resolve_queries();

        let session_id = match self.initialize(&token).await {
            Some(id) => id,
            None => return Vec::new(),
        };

        self.notify_initialized(&token, &session_id).await;

        let mut all_urls = Vec::new();
        for (idx, query) in queries.iter().enumerate() {
            let id = (idx as i64) + 2;
            if let Some(text) = self.call_web_search(&token, &session_id, id, query).await {
                all_urls.extend(extract_subscription_urls(&text));
            }
        }

        // Dedup across all queries.
        let mut seen = HashSet::new();
        all_urls.retain(|url| seen.insert(url.clone()));
        all_urls
    }
}

/// Parse the JSON-RPC payload out of an MCP response body.
///
/// Handles Server-Sent-Event framing (`event:` / `data:` lines, with an event
/// terminated by a blank line and multiple `data:` lines within one event
/// joined by `\n` per the SSE spec) as well as a plain `application/json` body
/// with no framing. Returns the last frame that carries a `result` or `error`
/// field, falling back to the last parseable frame, then to the whole body
/// parsed as JSON. Returns `None` if nothing parses.
pub(crate) fn parse_sse_data(body: &str) -> Option<serde_json::Value> {
    let mut chosen: Option<serde_json::Value> = None;
    let mut fallback: Option<serde_json::Value> = None;
    let mut buf = String::new();

    fn record(
        value: serde_json::Value,
        chosen: &mut Option<serde_json::Value>,
        fallback: &mut Option<serde_json::Value>,
    ) {
        if value.get("result").is_some() || value.get("error").is_some() {
            *chosen = Some(value);
        } else {
            *fallback = Some(value);
        }
    }

    fn flush(
        buf: &mut String,
        chosen: &mut Option<serde_json::Value>,
        fallback: &mut Option<serde_json::Value>,
    ) {
        if buf.is_empty() {
            return;
        }
        // Prefer parsing the accumulated payload as one JSON value (SSE joins
        // multiple `data:` lines of one event with `\n`); if that fails, treat
        // each line as an independent JSON frame.
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(buf) {
            record(value, chosen, fallback);
        } else {
            for line in buf.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                    record(value, chosen, fallback);
                }
            }
        }
        buf.clear();
    }

    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("data:") {
            let payload = rest.strip_prefix(' ').unwrap_or(rest);
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(payload);
        } else if trimmed.is_empty() {
            flush(&mut buf, &mut chosen, &mut fallback);
        }
        // `event:`, `id:`, and `:`-comment lines are ignored.
    }
    flush(&mut buf, &mut chosen, &mut fallback);

    if chosen.is_some() {
        return chosen;
    }
    if fallback.is_some() {
        return fallback;
    }
    // Fallback: a plain application/json body with no SSE framing.
    serde_json::from_str::<serde_json::Value>(body.trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_data_single_line() {
        let body = r#"data: {"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
        let value = parse_sse_data(body).expect("should parse");
        assert_eq!(value["result"]["ok"], serde_json::json!(true));
    }

    #[test]
    fn test_parse_sse_data_multiline_event() {
        let body =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"value\":42}}\n\n";
        let value = parse_sse_data(body).expect("should parse");
        assert_eq!(value["result"]["value"], serde_json::json!(42));
    }

    #[test]
    fn test_parse_sse_data_no_data_line() {
        let body = "event: message\n\n: comment only\n";
        assert!(parse_sse_data(body).is_none());
    }

    #[test]
    fn test_parse_sse_data_invalid_json() {
        let body = "data: {not valid json";
        assert!(parse_sse_data(body).is_none());
    }

    #[test]
    fn test_parse_sse_data_with_error_field() {
        let body = r#"data: {"jsonrpc":"2.0","id":3,"error":{"code":-1,"message":"oops"}}"#;
        let value = parse_sse_data(body).expect("should parse error response");
        assert!(value.get("error").is_some());
    }

    #[test]
    fn test_parse_sse_data_multiple_data_lines() {
        let body = "data: {\"interim\":1}\ndata: {\"interim\":2}\ndata: {\"result\":{\"ok\":true}}";
        let value = parse_sse_data(body).expect("should return line with result");
        assert_eq!(value["result"]["ok"], serde_json::json!(true));
    }

    #[test]
    fn test_parse_sse_data_empty_data_field() {
        let body = "data: \ndata: {\"jsonrpc\":\"2.0\",\"id\":4,\"result\":{\"value\":\"test\"}}";
        let value = parse_sse_data(body).expect("should skip empty data line");
        assert_eq!(value["result"]["value"], serde_json::json!("test"));
    }

    #[test]
    fn test_parse_sse_data_fallback_to_last() {
        let body = "data: {\"interim\":1}\ndata: {\"status\":\"processing\"}";
        let value = parse_sse_data(body).expect("should return last parsed value");
        assert_eq!(value["status"], serde_json::json!("processing"));
    }

    #[test]
    fn test_parse_sse_data_plain_json_body() {
        // No SSE framing — a plain application/json response.
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
        let value = parse_sse_data(body).expect("should parse plain json");
        assert_eq!(value["result"]["ok"], serde_json::json!(true));
    }

    #[test]
    fn test_parse_sse_data_prefers_result_over_progress() {
        // A progress frame (no result) then the real result frame in a later
        // event: the result-bearing frame must win.
        let body = "data: {\"progress\":50}\n\ndata: {\"result\":{\"done\":true}}\n\n";
        let value = parse_sse_data(body).expect("should pick result frame");
        assert_eq!(value["result"]["done"], serde_json::json!(true));
    }

    #[test]
    fn test_extract_from_grok_response_blob() {
        let blob = "Here are some links I found:\n\
            https://raw.githubusercontent.com/x/y/main/clash.yaml is a clash config,\n\
            and a panel at https://foo.com/api/v1/client/subscribe?token=abcdef1234567890.\n\
            Some noise: visit https://example.com/about for details.";
        let urls = extract_subscription_urls(blob);
        assert!(
            urls.contains(&"https://raw.githubusercontent.com/x/y/main/clash.yaml".to_string()),
            "expected raw clash.yaml URL to be extracted, got: {urls:?}"
        );
        assert!(
            urls.iter()
                .any(|u| u == "https://foo.com/api/v1/client/subscribe?token=abcdef1234567890"),
            "expected clean subscription URL without trailing period, got: {urls:?}"
        );
    }

    #[test]
    fn test_new_does_not_panic() {
        let disc = SearchDiscover::new(SearchConfig {
            mcp_url: "https://example.com/mcp".into(),
            auth_token: String::new(),
            tool_name: String::new(),
            queries: Vec::new(),
            max_queries: 3,
            timeout_sec: 30,
        });
        assert_eq!(disc.name(), "search");
    }

    #[test]
    fn test_resolve_queries_truncates() {
        let disc = SearchDiscover::new(SearchConfig {
            mcp_url: "https://example.com/mcp".into(),
            auth_token: "x".into(),
            tool_name: String::new(),
            queries: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            max_queries: 2,
            timeout_sec: 30,
        });
        assert_eq!(
            disc.resolve_queries(),
            vec!["a".to_string(), "b".to_string()]
        );
    }
}
