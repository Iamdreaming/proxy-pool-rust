//! Domain-based routing: maps a request host to an exit group by longest-suffix match.

use std::collections::HashMap;
use std::path::Path;

/// Result of matching a host against the configured route suffix table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteMatch {
    /// The selected routing group.
    pub group: String,
    /// The matched suffix rule, or `default` when no suffix matched.
    pub matched_rule: String,
    /// Whether this match came from the configured default group.
    pub is_default: bool,
}

/// Maps a request host to an exit group by longest-suffix domain match.
pub struct Router {
    /// (suffix_domain, group) pairs, sorted by suffix length descending.
    suffixes: Vec<(String, String)>,
    /// The fallback group used when nothing matches.
    default_group: String,
    /// Per-group scene hint (e.g. "latency", "bandwidth", "balanced").
    scenes: HashMap<String, String>,
}

impl Router {
    /// Build a router from a groups map.
    ///
    /// Each group lists domain suffixes. A bare domain like `github.com`
    /// matches that host and any subdomain. A `*.cn` entry matches any host
    /// ending in `.cn`. The special entry `default` marks the fallback group.
    pub fn new(groups: HashMap<String, Vec<String>>) -> Result<Self, String> {
        let mut suffixes = Vec::new();
        let mut default_group = None;
        let scenes = HashMap::new();

        for (group, entries) in &groups {
            for entry in entries {
                let e = entry.trim().to_lowercase();
                if e == "default" {
                    default_group = Some(group.clone());
                    continue;
                }
                let suffix = if let Some(s) = e.strip_prefix("*.") {
                    s.to_string()
                } else {
                    e
                };
                suffixes.push((suffix, group.clone()));
            }
        }

        let default_group =
            default_group.ok_or("routes must declare a 'default' entry in some group")?;

        // Sort by suffix length descending for longest-match-first
        suffixes.sort_by_key(|a| std::cmp::Reverse(a.0.len()));

        Ok(Self {
            suffixes,
            default_group,
            scenes,
        })
    }

    /// Load from a YAML file.
    pub fn from_yaml(path: impl AsRef<Path>) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read routes: {e}"))?;
        let data: serde_yaml::Value =
            serde_yaml::from_str(&text).map_err(|e| format!("parse routes YAML: {e}"))?;
        let groups: HashMap<String, Vec<String>> = data
            .get("groups")
            .and_then(|v| serde_yaml::from_value(v.clone()).ok())
            .unwrap_or_default();
        Self::new(groups)
    }

    /// Match a host to its routing group.
    pub fn match_group(&self, host: &str) -> &str {
        let host = normalize_host(host);

        for (suffix, group) in &self.suffixes {
            if host == *suffix || host.ends_with(&format!(".{suffix}")) {
                return group;
            }
        }
        &self.default_group
    }

    /// Match a host to its routing group with route-rule diagnostics.
    pub fn match_route(&self, host: &str) -> RouteMatch {
        let host = normalize_host(host);

        for (suffix, group) in &self.suffixes {
            if host == *suffix || host.ends_with(&format!(".{suffix}")) {
                return RouteMatch {
                    group: group.clone(),
                    matched_rule: suffix.clone(),
                    is_default: false,
                };
            }
        }

        RouteMatch {
            group: self.default_group.clone(),
            matched_rule: "default".into(),
            is_default: true,
        }
    }

    /// Get the scene hint for a group.
    pub fn scene_for(&self, group: &str) -> Option<&str> {
        self.scenes.get(group).map(|s| s.as_str())
    }
}

fn normalize_host(host: &str) -> String {
    let host = host.trim().to_lowercase();
    let host = host.split(':').next().unwrap_or(&host);
    host.trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_match() {
        let mut groups = HashMap::new();
        groups.insert("direct".into(), vec!["*.cn".into(), "default".into()]);
        groups.insert("warp".into(), vec!["google.com".into()]);
        groups.insert("free_pool".into(), vec!["github.com".into()]);

        let router = Router::new(groups).unwrap();

        assert_eq!(router.match_group("example.cn"), "direct");
        assert_eq!(router.match_group("sub.example.cn"), "direct");
        assert_eq!(router.match_group("google.com"), "warp");
        assert_eq!(router.match_group("api.google.com"), "warp");
        assert_eq!(router.match_group("github.com"), "free_pool");
        assert_eq!(router.match_group("unknown.com"), "direct"); // default
    }

    #[test]
    fn test_router_match_route_includes_rule() {
        let mut groups = HashMap::new();
        groups.insert("direct".into(), vec!["*.cn".into(), "default".into()]);
        groups.insert("free_pool".into(), vec!["github.com".into()]);

        let router = Router::new(groups).unwrap();

        let matched = router.match_route("api.github.com");
        assert_eq!(matched.group, "free_pool");
        assert_eq!(matched.matched_rule, "github.com");
        assert!(!matched.is_default);

        let default = router.match_route("unknown.example");
        assert_eq!(default.group, "direct");
        assert_eq!(default.matched_rule, "default");
        assert!(default.is_default);
    }
}
