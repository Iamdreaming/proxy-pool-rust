//! GitHub search discoverer: searches GitHub for subscription file URLs.
//!
//! Uses the GitHub Search API to discover proxy subscription files in two ways:
//!
//! 1. **Repository search** — finds repos that likely contain subscription files,
//!    then generates candidate raw URLs for well-known filenames.
//! 2. **Code search** — finds specific files matching keywords, then converts
//!    their GitHub page URLs to raw content URLs.

use crate::discover::Discover;

/// Configuration for [`GitHubSearchDiscover`].
#[derive(Debug, Clone)]
pub struct GitHubSearchConfig {
    /// Optional GitHub personal access token for higher rate limits.
    pub token: Option<String>,
    /// Maximum number of results per search page.
    pub max_results: u32,
    /// Keywords to search for (each triggers a repo + code search).
    pub keywords: Vec<String>,
    /// HTTP request timeout in seconds.
    pub timeout_sec: u64,
}

/// Well-known subscription filenames to probe in discovered repositories.
const SUB_FILENAMES: &[&str] = &["clash.yaml", "proxy.yaml", "v2ray.yaml", "sub.yaml"];

/// A discoverer that searches GitHub for proxy subscription files.
pub struct GitHubSearchDiscover {
    config: GitHubSearchConfig,
    client: reqwest::Client,
}

impl GitHubSearchDiscover {
    /// Create a new GitHub search discoverer with the given configuration.
    pub fn new(config: GitHubSearchConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_sec))
            .user_agent("proxy-pool-rust")
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("failed to build reqwest client for GitHub search: {e}");
                reqwest::Client::new()
            });
        Self { config, client }
    }

    /// Build a request builder with optional auth headers.
    fn request(&self, url: &str) -> reqwest::RequestBuilder {
        let mut req = self.client.get(url);
        if let Some(token) = &self.config.token {
            req = req
                .header("Authorization", format!("Bearer {token}"))
                .header("Accept", "application/vnd.github+json");
        } else {
            req = req.header("Accept", "application/vnd.github+json");
        }
        req
    }

    /// Search GitHub repositories for the given keyword and return candidate URLs.
    async fn search_repos(&self, keyword: &str) -> Vec<String> {
        let url = format!(
            "https://api.github.com/search/repositories?q={keyword}&sort=updated&order=desc&per_page={}",
            self.config.max_results,
        );
        let resp = match self.request(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(name = self.name(), %keyword, "repo search request failed: {e}");
                return Vec::new();
            }
        };

        let status = resp.status();
        if status.as_u16() == 403 || status.as_u16() == 429 {
            tracing::warn!(name = self.name(), %keyword, %status, "GitHub rate limited");
            return Vec::new();
        }

        let body = match resp.json::<serde_json::Value>().await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(name = self.name(), %keyword, "repo search JSON parse failed: {e}");
                return Vec::new();
            }
        };

        let mut urls = Vec::new();
        if let Some(items) = body.get("items").and_then(|v| v.as_array()) {
            for repo in items {
                let full_name = match repo.get("full_name").and_then(|v| v.as_str()) {
                    Some(n) => n,
                    None => continue,
                };
                let branch = repo
                    .get("default_branch")
                    .and_then(|v| v.as_str())
                    .unwrap_or("main");
                for fname in SUB_FILENAMES {
                    urls.push(format!(
                        "https://raw.githubusercontent.com/{full_name}/{branch}/{fname}"
                    ));
                }
            }
        }
        urls
    }

    /// Search GitHub code for the given keyword and return candidate raw URLs.
    async fn search_code(&self, keyword: &str) -> Vec<String> {
        let url = format!(
            "https://api.github.com/search/code?q={keyword}&sort=updated&order=desc&per_page={}",
            self.config.max_results,
        );
        let resp = match self.request(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(name = self.name(), %keyword, "code search request failed: {e}");
                return Vec::new();
            }
        };

        let status = resp.status();
        if status.as_u16() == 403 || status.as_u16() == 429 {
            tracing::warn!(name = self.name(), %keyword, %status, "GitHub rate limited");
            return Vec::new();
        }

        let body = match resp.json::<serde_json::Value>().await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(name = self.name(), %keyword, "code search JSON parse failed: {e}");
                return Vec::new();
            }
        };

        let mut urls = Vec::new();
        if let Some(items) = body.get("items").and_then(|v| v.as_array()) {
            for item in items {
                if let Some(html_url) = item.get("html_url").and_then(|v| v.as_str()) {
                    urls.push(github_to_raw_url(html_url));
                }
            }
        }
        urls
    }
}

#[async_trait::async_trait]
impl Discover for GitHubSearchDiscover {
    fn name(&self) -> &str {
        "github_search"
    }

    async fn discover(&self) -> Vec<String> {
        let mut all_urls = Vec::new();

        for keyword in &self.config.keywords {
            let repo_urls = self.search_repos(keyword).await;
            all_urls.extend(repo_urls);

            let code_urls = self.search_code(keyword).await;
            all_urls.extend(code_urls);
        }

        // Dedup
        let mut seen = std::collections::HashSet::new();
        all_urls.retain(|url| seen.insert(url.clone()));

        all_urls
    }
}

/// Convert a GitHub page URL to a raw content URL.
///
/// Transforms:
/// `https://github.com/user/repo/blob/main/clash.yaml`
/// into
/// `https://raw.githubusercontent.com/user/repo/main/clash.yaml`
///
/// If the URL does not contain `/blob/` it is returned unchanged (already raw).
pub fn github_to_raw_url(url: &str) -> String {
    let github_prefix = "https://github.com";
    if let Some(idx) = url.find("/blob/") {
        let suffix = &url[idx + "/blob/".len()..];
        // Slice past "https://github.com" to get "/user/repo", then append branch/path.
        let path = &url[github_prefix.len()..idx];
        format!("https://raw.githubusercontent.com{path}/{suffix}")
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_to_raw_url() {
        let input = "https://github.com/user/repo/blob/main/clash.yaml";
        let expected = "https://raw.githubusercontent.com/user/repo/main/clash.yaml";
        assert_eq!(github_to_raw_url(input), expected);
    }

    #[test]
    fn test_github_to_raw_url_no_blob() {
        let input = "https://raw.githubusercontent.com/user/repo/main/clash.yaml";
        // Already a raw URL — should be returned unchanged.
        assert_eq!(github_to_raw_url(input), input);
    }

    #[test]
    fn test_github_to_raw_url_different_branch() {
        let input = "https://github.com/org/proxy-list/blob/develop/sub.yaml";
        let expected = "https://raw.githubusercontent.com/org/proxy-list/develop/sub.yaml";
        assert_eq!(github_to_raw_url(input), expected);
    }
}
