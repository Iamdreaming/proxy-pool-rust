//! Domain-based routing: maps a request host to an exit group by longest-suffix match.
//!
//! Groups may declare a quality tier (`any` / `standard` / `premium`) that drives
//! gateway exit order. Legacy YAML (`groups.name: [suffix…]`) remains supported.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Quality tier controlling gateway exit preference order.
///
/// Exit order is defined by the route selector (`exits_for_tier`); this enum is the
/// YAML / diagnostics contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityTier {
    /// Low barrier: free pool first, may borrow warp/xray.
    Any,
    /// Prefer encrypted/WARP, free pool still allowed later.
    Standard,
    /// High barrier: xray/warp only — never free pool.
    Premium,
}

impl QualityTier {
    /// Stable snake_case label used in YAML and `RouteDecision.tier`.
    pub fn as_str(self) -> &'static str {
        match self {
            QualityTier::Any => "any",
            QualityTier::Standard => "standard",
            QualityTier::Premium => "premium",
        }
    }

    /// Parse a tier string; unknown values fail fast at config load.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "any" => Ok(QualityTier::Any),
            "standard" => Ok(QualityTier::Standard),
            "premium" => Ok(QualityTier::Premium),
            other => Err(format!(
                "unknown quality tier '{other}' (expected any|standard|premium)"
            )),
        }
    }
}

impl std::fmt::Display for QualityTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Default tier when a group omits `tier` in YAML (R3).
///
/// - `direct` → `None` (Direct-only special path)
/// - `free_pool` → `any`
/// - `warp` / `xray` → `premium`
/// - other custom groups → `any` (safe default)
pub fn default_tier_for_group(group: &str) -> Option<QualityTier> {
    match group {
        "direct" => None,
        "free_pool" => Some(QualityTier::Any),
        "warp" | "xray" => Some(QualityTier::Premium),
        _ => Some(QualityTier::Any),
    }
}

/// Known route exit names accepted in YAML `exits` overrides.
const KNOWN_EXIT_NAMES: &[&str] = &["direct", "free_pool", "warp", "xray", "no_proxy"];

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
#[derive(Debug)]
pub struct Router {
    /// (suffix_domain, group) pairs, sorted by suffix length descending.
    suffixes: Vec<(String, String)>,
    /// The fallback group used when nothing matches.
    default_group: String,
    /// Per-group scene hint (e.g. "latency", "bandwidth", "balanced").
    scenes: HashMap<String, String>,
    /// Resolved quality tier per group (`direct` typically omitted).
    group_tiers: HashMap<String, QualityTier>,
    /// Optional explicit exit order override (snake_case exit names).
    group_exit_overrides: HashMap<String, Vec<String>>,
}

impl Router {
    /// Build a router from a groups map (legacy domain lists only).
    ///
    /// Each group lists domain suffixes. A bare domain like `github.com`
    /// matches that host and any subdomain. A `*.cn` entry matches any host
    /// ending in `.cn`. The special entry `default` marks the fallback group.
    ///
    /// Tiers are assigned via [`default_tier_for_group`].
    pub fn new(groups: HashMap<String, Vec<String>>) -> Result<Self, String> {
        let mut group_tiers = HashMap::new();
        for group in groups.keys() {
            if let Some(tier) = default_tier_for_group(group) {
                group_tiers.insert(group.clone(), tier);
            }
        }
        Self::from_parts(groups, group_tiers, HashMap::new(), HashMap::new())
    }

    /// Load from a YAML file (legacy list or extended mapping form).
    pub fn from_yaml(path: impl AsRef<Path>) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read routes: {e}"))?;
        Self::from_yaml_str(&text)
    }

    /// Parse routes YAML text (legacy list or extended mapping form).
    pub fn from_yaml_str(text: &str) -> Result<Self, String> {
        let data: serde_yaml::Value =
            serde_yaml::from_str(text).map_err(|e| format!("parse routes YAML: {e}"))?;
        let empty_groups = serde_yaml::Value::Mapping(Default::default());
        let groups_val = data.get("groups").unwrap_or(&empty_groups);
        let mapping = groups_val
            .as_mapping()
            .ok_or("routes.groups must be a mapping")?;

        let mut groups: HashMap<String, Vec<String>> = HashMap::new();
        let mut group_tiers: HashMap<String, QualityTier> = HashMap::new();
        let mut group_exit_overrides: HashMap<String, Vec<String>> = HashMap::new();
        let mut scenes: HashMap<String, String> = HashMap::new();

        for (key, value) in mapping {
            let name = key
                .as_str()
                .ok_or("routes.groups keys must be strings")?
                .to_string();

            let parsed = parse_group_value(&name, value)?;
            groups.insert(name.clone(), parsed.domains);

            let tier = parsed
                .explicit_tier
                .or_else(|| default_tier_for_group(&name));
            if let Some(t) = tier {
                group_tiers.insert(name.clone(), t);
            }

            if let Some(exit_list) = parsed.exits {
                validate_exit_override(&name, tier, &exit_list)?;
                group_exit_overrides.insert(name.clone(), exit_list);
            }

            if let Some(scene) = parsed.scene {
                scenes.insert(name, scene);
            }
        }

        Self::from_parts(groups, group_tiers, group_exit_overrides, scenes)
    }

    fn from_parts(
        groups: HashMap<String, Vec<String>>,
        group_tiers: HashMap<String, QualityTier>,
        group_exit_overrides: HashMap<String, Vec<String>>,
        scenes: HashMap<String, String>,
    ) -> Result<Self, String> {
        let mut suffixes = Vec::new();
        let mut default_group = None;

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
            group_tiers,
            group_exit_overrides,
        })
    }

    /// Match a host to its routing group.
    pub fn match_group(&self, host: &str) -> &str {
        self.find_suffix_match(host)
            .map(|(_, group)| group)
            .unwrap_or(&self.default_group)
    }

    /// Match a host to its routing group with route-rule diagnostics.
    pub fn match_route(&self, host: &str) -> RouteMatch {
        if let Some((suffix, group)) = self.find_suffix_match(host) {
            return RouteMatch {
                group: group.to_string(),
                matched_rule: suffix.to_string(),
                is_default: false,
            };
        }

        RouteMatch {
            group: self.default_group.clone(),
            matched_rule: "default".into(),
            is_default: true,
        }
    }

    /// Longest-suffix match: `(suffix, group)` or `None` for default fallback.
    fn find_suffix_match(&self, host: &str) -> Option<(&str, &str)> {
        let host = normalize_host(host);
        for (suffix, group) in &self.suffixes {
            if host == *suffix || host.ends_with(&format!(".{suffix}")) {
                return Some((suffix.as_str(), group.as_str()));
            }
        }
        None
    }

    /// Get the scene hint for a group.
    pub fn scene_for(&self, group: &str) -> Option<&str> {
        self.scenes.get(group).map(|s| s.as_str())
    }

    /// Resolved quality tier for a group, if any (`direct` is typically `None`).
    pub fn tier_for(&self, group: &str) -> Option<QualityTier> {
        self.group_tiers.get(group).copied()
    }

    /// Optional explicit exit-order override (snake_case names) for a group.
    pub fn exit_override_for(&self, group: &str) -> Option<&[String]> {
        self.group_exit_overrides.get(group).map(|v| v.as_slice())
    }

    /// Whether this group is Direct-only (no quality tier, typically `direct`).
    pub fn is_direct_only(&self, group: &str) -> bool {
        self.tier_for(group).is_none() && self.exit_override_for(group).is_none()
    }
}

/// Parsed `groups.<name>` entry: domains, optional tier, exits override, scene.
struct ParsedGroupValue {
    domains: Vec<String>,
    explicit_tier: Option<QualityTier>,
    exits: Option<Vec<String>>,
    scene: Option<String>,
}

/// Parse one `groups.<name>` value: either a domain sequence or an extended mapping.
fn parse_group_value(name: &str, value: &serde_yaml::Value) -> Result<ParsedGroupValue, String> {
    if value.as_sequence().is_some() {
        return Ok(ParsedGroupValue {
            domains: parse_string_list(value, &format!("groups.{name}"))?,
            explicit_tier: None,
            exits: None,
            scene: None,
        });
    }

    let map = value.as_mapping().ok_or_else(|| {
        format!("groups.{name}: must be a domain list or a mapping with 'domains'")
    })?;

    let domains_val = yaml_map_get(map, "domains")
        .ok_or_else(|| format!("groups.{name}: extended form requires 'domains' list"))?;
    let domains = parse_string_list(domains_val, &format!("groups.{name}.domains"))?;

    let explicit_tier = match yaml_map_get(map, "tier") {
        Some(v) => {
            let s = v
                .as_str()
                .ok_or_else(|| format!("groups.{name}.tier must be a string"))?;
            Some(QualityTier::parse(s).map_err(|e| format!("groups.{name}: {e}"))?)
        }
        None => None,
    };

    let exits = match yaml_map_get(map, "exits") {
        Some(v) => {
            let list = parse_string_list(v, &format!("groups.{name}.exits"))?
                .into_iter()
                .map(|s| s.to_ascii_lowercase())
                .collect::<Vec<_>>();
            if list.is_empty() {
                return Err(format!("groups.{name}.exits must be non-empty when set"));
            }
            for exit in &list {
                if !KNOWN_EXIT_NAMES.contains(&exit.as_str()) {
                    return Err(format!(
                        "groups.{name}.exits: unknown exit '{exit}' (expected one of {})",
                        KNOWN_EXIT_NAMES.join(", ")
                    ));
                }
            }
            Some(list)
        }
        None => None,
    };

    let scene = yaml_map_get(map, "scene")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(ParsedGroupValue {
        domains,
        explicit_tier,
        exits,
        scene,
    })
}

fn yaml_map_get<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    map.get(serde_yaml::Value::String(key.into()))
}

fn parse_string_list(value: &serde_yaml::Value, label: &str) -> Result<Vec<String>, String> {
    value
        .as_sequence()
        .ok_or_else(|| format!("{label} must be a list"))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| format!("{label} entries must be strings"))
        })
        .collect()
}

fn validate_exit_override(
    name: &str,
    tier: Option<QualityTier>,
    exits: &[String],
) -> Result<(), String> {
    // D2: premium hard boundary — reject free_pool in explicit exits.
    if tier == Some(QualityTier::Premium) && exits.iter().any(|e| e == "free_pool") {
        return Err(format!(
            "groups.{name}: tier=premium cannot include free_pool in exits (D2 hard boundary)"
        ));
    }
    Ok(())
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

    #[test]
    fn default_tier_mapping_matches_r3() {
        assert_eq!(default_tier_for_group("direct"), None);
        assert_eq!(default_tier_for_group("free_pool"), Some(QualityTier::Any));
        assert_eq!(default_tier_for_group("warp"), Some(QualityTier::Premium));
        assert_eq!(default_tier_for_group("xray"), Some(QualityTier::Premium));
        assert_eq!(default_tier_for_group("openai"), Some(QualityTier::Any));
        assert_eq!(default_tier_for_group("custom"), Some(QualityTier::Any));
    }

    #[test]
    fn legacy_yaml_loads_with_default_tiers() {
        let yaml = r#"
groups:
  direct:
    - "*.cn"
    - default
  free_pool:
    - "github.com"
  warp:
    - "cloudflare.com"
"#;
        let router = Router::from_yaml_str(yaml).unwrap();
        assert_eq!(router.tier_for("free_pool"), Some(QualityTier::Any));
        assert_eq!(router.tier_for("warp"), Some(QualityTier::Premium));
        assert_eq!(router.tier_for("direct"), None);
        assert!(router.is_direct_only("direct"));
        assert_eq!(router.match_group("api.github.com"), "free_pool");
        assert_eq!(router.match_group("unknown.test"), "direct");
    }

    #[test]
    fn extended_yaml_parses_tier_and_domains() {
        let yaml = r#"
groups:
  direct:
    domains:
      - "*.cn"
      - default
  free_pool:
    tier: any
    domains:
      - "github.com"
  openai:
    tier: premium
    domains:
      - "openai.com"
      - "chatgpt.com"
"#;
        let router = Router::from_yaml_str(yaml).unwrap();
        assert_eq!(router.tier_for("openai"), Some(QualityTier::Premium));
        assert_eq!(router.tier_for("free_pool"), Some(QualityTier::Any));
        assert_eq!(router.match_group("api.openai.com"), "openai");
        assert_eq!(router.match_group("chatgpt.com"), "openai");
        assert!(router.exit_override_for("openai").is_none());
    }

    #[test]
    fn extended_yaml_exit_override_stored() {
        let yaml = r#"
groups:
  direct:
    domains:
      - default
  custom:
    tier: standard
    domains:
      - "example.com"
    exits:
      - xray
      - warp
      - free_pool
      - no_proxy
"#;
        let router = Router::from_yaml_str(yaml).unwrap();
        assert_eq!(
            router.exit_override_for("custom").unwrap(),
            &[
                "xray".to_string(),
                "warp".into(),
                "free_pool".into(),
                "no_proxy".into()
            ][..]
        );
        assert_eq!(router.tier_for("custom"), Some(QualityTier::Standard));
    }

    #[test]
    fn reject_unknown_tier() {
        let yaml = r#"
groups:
  direct:
    domains: [default]
  bad:
    tier: gold
    domains: ["x.com"]
"#;
        let err = Router::from_yaml_str(yaml).unwrap_err();
        assert!(err.contains("unknown quality tier"), "{err}");
    }

    #[test]
    fn reject_premium_with_free_pool_override() {
        let yaml = r#"
groups:
  direct:
    domains: [default]
  openai:
    tier: premium
    domains: ["openai.com"]
    exits: [xray, free_pool, no_proxy]
"#;
        let err = Router::from_yaml_str(yaml).unwrap_err();
        assert!(
            err.contains("premium") && err.contains("free_pool"),
            "{err}"
        );
    }

    #[test]
    fn reject_unknown_exit_name() {
        let yaml = r#"
groups:
  direct:
    domains: [default]
  custom:
    tier: any
    domains: ["a.com"]
    exits: [warp, vpn]
"#;
        let err = Router::from_yaml_str(yaml).unwrap_err();
        assert!(err.contains("unknown exit"), "{err}");
    }

    #[test]
    fn reject_empty_exits_override() {
        let yaml = r#"
groups:
  direct:
    domains: [default]
  custom:
    tier: any
    domains: ["a.com"]
    exits: []
"#;
        let err = Router::from_yaml_str(yaml).unwrap_err();
        assert!(err.contains("non-empty"), "{err}");
    }

    #[test]
    fn quality_tier_serde_snake_case() {
        assert_eq!(
            serde_json::to_string(&QualityTier::Premium).unwrap(),
            "\"premium\""
        );
        assert_eq!(
            serde_json::from_str::<QualityTier>("\"standard\"").unwrap(),
            QualityTier::Standard
        );
    }

    #[test]
    fn example_routes_yaml_loads_with_expected_tiers() {
        // Relative to crate CARGO_MANIFEST_DIR → workspace config/.
        // Primary profile: overseas-stable (default → premium overseas group).
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/routes.example.yaml");
        let router = Router::from_yaml(&path).expect("routes.example.yaml should parse");
        assert_eq!(router.tier_for("direct"), None);
        assert!(router.is_direct_only("direct"));
        assert_eq!(router.tier_for("overseas"), Some(QualityTier::Premium));
        assert_eq!(router.tier_for("free_pool"), Some(QualityTier::Any));
        assert_eq!(router.tier_for("warp"), Some(QualityTier::Premium));
        assert_eq!(router.match_group("api.github.com"), "free_pool");
        assert_eq!(router.match_group("cloudflare.com"), "warp");
        assert_eq!(router.match_group("foo.cn"), "direct");
        assert_eq!(router.match_group("unknown.example"), "overseas");
    }
}
