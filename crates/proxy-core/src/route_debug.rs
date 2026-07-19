//! Traceable gateway route selection and route diagnostics.

use crate::capability::{CapabilityStore, CapabilityTag};
use crate::circuit;
use crate::geoip::GeoIPLookup;
use crate::models::{EncryptedProxyState, Protocol, Proxy, WarpInstance};
use crate::router::{QualityTier, RouteMatch, Router};
use crate::store::ProxyStore;
use crate::warp::balancer::WarpBalancer;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

/// Fresh success window for xray route eligibility (Decision D2: 15 minutes).
const XRAY_ROUTE_FRESH_SUCCESS_SECS: i64 = 900;

const BUSINESS_OVERSEAS_DOMAINS: &[&str] = &[
    "openai.com",
    "chatgpt.com",
    "reddit.com",
    "discord.com",
    "x.com",
    "twitter.com",
];

const DIRECT_REACHABLE_DOMAINS: &[&str] = &["github.com", "news.ycombinator.com"];

/// Runtime upstream selected for a gateway request.
#[derive(Debug, Clone)]
pub enum Upstream {
    /// Connect directly to the target.
    Direct,
    /// Route through a pool proxy.
    Proxy(Proxy),
    /// Route through a WARP instance.
    Warp {
        id: u32,
        socks5_host: String,
        socks5_port: u16,
    },
    /// Route through an xray-node local SOCKS5 port.
    Xray { local_socks5_port: u16 },
    /// Chain through a pool proxy and then WARP.
    WarpChain {
        proxy: Proxy,
        socks5_host: String,
        socks5_port: u16,
    },
    /// No upstream is available.
    NoProxy,
}

/// Stable route exit categories used in JSON and metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteExit {
    Direct,
    FreePool,
    Warp,
    Xray,
    NoProxy,
}

impl RouteExit {
    const ALL: [RouteExit; 5] = [
        RouteExit::Direct,
        RouteExit::FreePool,
        RouteExit::Warp,
        RouteExit::Xray,
        RouteExit::NoProxy,
    ];

    fn label(self) -> &'static str {
        match self {
            RouteExit::Direct => "direct",
            RouteExit::FreePool => "free_pool",
            RouteExit::Warp => "warp",
            RouteExit::Xray => "xray",
            RouteExit::NoProxy => "no_proxy",
        }
    }

    /// Parse a snake_case exit name from YAML / diagnostics.
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "direct" => Some(RouteExit::Direct),
            "free_pool" => Some(RouteExit::FreePool),
            "warp" => Some(RouteExit::Warp),
            "xray" => Some(RouteExit::Xray),
            "no_proxy" => Some(RouteExit::NoProxy),
            _ => None,
        }
    }

    fn metric_index(self) -> usize {
        match self {
            RouteExit::Direct => 0,
            RouteExit::FreePool => 1,
            RouteExit::Warp => 2,
            RouteExit::Xray => 3,
            RouteExit::NoProxy => 4,
        }
    }
}

/// Route decision candidate exposed to operators.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteCandidate {
    /// Exit type represented by this candidate.
    pub exit: RouteExit,
    /// Candidate priority in the fallback order.
    pub priority: usize,
    /// Why this candidate is in the route plan.
    pub source: String,
    /// Whether an upstream resource was available at selection time.
    pub available: bool,
    /// Human-readable availability or skip reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Non-sensitive selected endpoint detail, when useful.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// An unavailable route exit with the reason it was skipped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteUnavailable {
    /// Exit type that was unavailable.
    pub exit: RouteExit,
    /// Human-readable skip reason.
    pub reason: String,
}

/// GeoIP contribution to a route decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteGeoIpDecision {
    /// ISO country code returned by GeoIP.
    pub country: String,
    /// Human-readable country name.
    pub country_name: String,
    /// Whether the target is considered overseas.
    pub overseas: bool,
}

/// Operator-facing explanation of a gateway route decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteDecision {
    /// Target host evaluated by the selector.
    pub host: String,
    /// Requested proxy protocol used for pool lookup.
    pub protocol: String,
    /// Matched route group, if a router was configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_group: Option<String>,
    /// Matched suffix rule or `default`, if a router was configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_rule: Option<String>,
    /// Resolved quality tier (`any` / `standard` / `premium`), when applicable.
    ///
    /// `null`/omitted for Direct-only groups and paths that do not use tier tables
    /// (e.g. some hardcoded domain helpers).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// High-level reason for the candidate order.
    pub matched_reason: String,
    /// GeoIP decision, present only when GeoIP was consulted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geoip: Option<RouteGeoIpDecision>,
    /// Ordered route candidates.
    pub candidates: Vec<RouteCandidate>,
    /// First available exit selected by the selector.
    pub selected: RouteExit,
    /// Unavailable candidates and reasons.
    pub unavailable: Vec<RouteUnavailable>,
}

/// Concrete candidate used by gateway handlers for runtime connection attempts.
#[derive(Debug, Clone)]
pub struct RouteUpstreamCandidate {
    /// Exit type represented by this candidate.
    pub exit: RouteExit,
    /// Concrete upstream target.
    pub upstream: Upstream,
    /// Non-sensitive endpoint detail for logging.
    pub detail: Option<String>,
}

/// Full route selection result: runtime upstream plus operator diagnostics.
#[derive(Debug, Clone)]
pub struct RouteSelection {
    /// First available upstream, preserving the old selector contract.
    pub upstream: Upstream,
    /// Operator-facing decision explanation.
    pub decision: RouteDecision,
    /// Concrete available candidates in fallback order.
    pub upstream_candidates: Vec<RouteUpstreamCandidate>,
}

/// Gateway protocol labels used for metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayRouteProtocol {
    /// HTTP CONNECT gateway path.
    HttpConnect,
    /// SOCKS5 gateway path.
    Socks5,
    /// Other selector use, such as route dry-run.
    Other,
}

impl GatewayRouteProtocol {
    const ALL: [GatewayRouteProtocol; 3] = [
        GatewayRouteProtocol::HttpConnect,
        GatewayRouteProtocol::Socks5,
        GatewayRouteProtocol::Other,
    ];

    fn label(self) -> &'static str {
        match self {
            GatewayRouteProtocol::HttpConnect => "http_connect",
            GatewayRouteProtocol::Socks5 => "socks5",
            GatewayRouteProtocol::Other => "other",
        }
    }

    fn metric_index(self) -> usize {
        match self {
            GatewayRouteProtocol::HttpConnect => 0,
            GatewayRouteProtocol::Socks5 => 1,
            GatewayRouteProtocol::Other => 2,
        }
    }
}

/// Gateway attempt status labels used for metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayAttemptStatus {
    /// A tunnel was established through this exit.
    Success,
    /// A concrete exit was available but the connection attempt failed.
    Failure,
    /// No usable upstream existed for this exit.
    Unavailable,
}

impl GatewayAttemptStatus {
    const ALL: [GatewayAttemptStatus; 3] = [
        GatewayAttemptStatus::Success,
        GatewayAttemptStatus::Failure,
        GatewayAttemptStatus::Unavailable,
    ];

    fn label(self) -> &'static str {
        match self {
            GatewayAttemptStatus::Success => "success",
            GatewayAttemptStatus::Failure => "failure",
            GatewayAttemptStatus::Unavailable => "unavailable",
        }
    }

    fn metric_index(self) -> usize {
        match self {
            GatewayAttemptStatus::Success => 0,
            GatewayAttemptStatus::Failure => 1,
            GatewayAttemptStatus::Unavailable => 2,
        }
    }
}

const METRIC_CELL_COUNT: usize = 3 * 5 * 3;
const FREE_POOL_CANDIDATE_LIMIT: usize = 8;
/// Gateway pool candidates are drawn from this many highest-scored proxies so a
/// large mass of low-score entries cannot dominate the weighted selection.
const POOL_TOP_CANDIDATE_POOL: usize = 50;
const POOL_PROXY_FAILURE_COOLDOWN: Duration = Duration::from_secs(300);
const XRAY_FAILURE_COOLDOWN: Duration = Duration::from_secs(300);

/// Process-local gateway route metrics.
pub struct GatewayRouteMetrics {
    attempts: [AtomicU64; METRIC_CELL_COUNT],
}

impl GatewayRouteMetrics {
    /// Create an empty gateway metrics registry.
    pub fn new() -> Self {
        Self {
            attempts: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    /// Record one gateway route attempt.
    pub fn record(
        &self,
        protocol: GatewayRouteProtocol,
        exit: RouteExit,
        status: GatewayAttemptStatus,
    ) {
        let idx = metric_index(protocol, exit, status);
        self.attempts[idx].fetch_add(1, Ordering::Relaxed);
    }

    /// Render Prometheus text metrics for gateway route attempts.
    pub fn render_prometheus(&self) -> String {
        let mut lines = String::new();
        lines.push_str(
            "# HELP proxy_gateway_route_attempts_total Gateway route attempts by protocol, exit, and status.\n",
        );
        lines.push_str("# TYPE proxy_gateway_route_attempts_total counter\n");
        for protocol in GatewayRouteProtocol::ALL {
            for exit in RouteExit::ALL {
                for status in GatewayAttemptStatus::ALL {
                    let value =
                        self.attempts[metric_index(protocol, exit, status)].load(Ordering::Relaxed);
                    lines.push_str(&format!(
                        "proxy_gateway_route_attempts_total{{protocol=\"{}\",exit=\"{}\",status=\"{}\"}} {}\n",
                        protocol.label(),
                        exit.label(),
                        status.label(),
                        value
                    ));
                }
            }
        }
        lines
    }
}

impl Default for GatewayRouteMetrics {
    fn default() -> Self {
        Self::new()
    }
}

fn metric_index(
    protocol: GatewayRouteProtocol,
    exit: RouteExit,
    status: GatewayAttemptStatus,
) -> usize {
    (protocol.metric_index() * RouteExit::ALL.len() * GatewayAttemptStatus::ALL.len())
        + (exit.metric_index() * GatewayAttemptStatus::ALL.len())
        + status.metric_index()
}

/// Selects gateway upstreams and produces route diagnostics.
pub struct UpstreamSelector {
    store: Arc<ProxyStore>,
    balancer: Option<Arc<WarpBalancer>>,
    router: Option<Arc<Router>>,
    geoip: Option<Arc<Mutex<GeoIPLookup>>>,
    metrics: Arc<GatewayRouteMetrics>,
    pool_proxy_failed_until: Arc<RwLock<HashMap<String, Instant>>>,
    xray_failed_until: Arc<RwLock<HashMap<u16, Instant>>>,
    cap_store: CapabilityStore,
}

impl UpstreamSelector {
    /// Build a selector with a new gateway metrics registry.
    pub fn new(
        store: Arc<ProxyStore>,
        balancer: Option<Arc<WarpBalancer>>,
        router: Option<Arc<Router>>,
        geoip: Option<Arc<Mutex<GeoIPLookup>>>,
    ) -> Self {
        Self::with_metrics(
            store,
            balancer,
            router,
            geoip,
            Arc::new(GatewayRouteMetrics::new()),
        )
    }

    /// Build a selector with an explicit gateway metrics registry.
    pub fn with_metrics(
        store: Arc<ProxyStore>,
        balancer: Option<Arc<WarpBalancer>>,
        router: Option<Arc<Router>>,
        geoip: Option<Arc<Mutex<GeoIPLookup>>>,
        metrics: Arc<GatewayRouteMetrics>,
    ) -> Self {
        let cap_store = CapabilityStore::new(store.raw_conn());
        Self {
            store,
            balancer,
            router,
            geoip,
            metrics,
            pool_proxy_failed_until: Arc::new(RwLock::new(HashMap::new())),
            xray_failed_until: Arc::new(RwLock::new(HashMap::new())),
            cap_store,
        }
    }

    /// Return the shared gateway metrics registry.
    pub fn metrics(&self) -> Arc<GatewayRouteMetrics> {
        self.metrics.clone()
    }

    /// Feed concrete gateway attempt outcomes back into route health.
    pub async fn record_upstream_attempt(&self, upstream: &Upstream, status: GatewayAttemptStatus) {
        match (upstream, status) {
            (
                Upstream::Warp {
                    id,
                    socks5_host,
                    socks5_port,
                },
                GatewayAttemptStatus::Failure,
            ) => {
                if let Some(balancer) = &self.balancer {
                    balancer.mark_failed(*id).await;
                    tracing::warn!(
                        warp_id = *id,
                        socks5_host = socks5_host.as_str(),
                        socks5_port = *socks5_port,
                        "gateway marked WARP instance unhealthy after connection failure"
                    );
                }
            }
            (Upstream::Proxy(proxy), GatewayAttemptStatus::Failure) => {
                let key = proxy.dedup_key();
                self.pool_proxy_failed_until
                    .write()
                    .await
                    .insert(key.clone(), Instant::now() + POOL_PROXY_FAILURE_COOLDOWN);
                tracing::debug!(
                    proxy = %key,
                    "gateway put pool proxy into failure cooldown"
                );
            }
            (Upstream::Proxy(proxy), GatewayAttemptStatus::Success) => {
                let key = proxy.dedup_key();
                self.pool_proxy_failed_until.write().await.remove(&key);
            }
            (Upstream::Xray { local_socks5_port }, GatewayAttemptStatus::Failure) => {
                self.xray_failed_until
                    .write()
                    .await
                    .insert(*local_socks5_port, Instant::now() + XRAY_FAILURE_COOLDOWN);
                tracing::debug!(
                    local_socks5_port = *local_socks5_port,
                    "gateway put xray node into failure cooldown"
                );
            }
            (Upstream::Xray { local_socks5_port }, GatewayAttemptStatus::Success) => {
                self.xray_failed_until
                    .write()
                    .await
                    .remove(local_socks5_port);
            }
            (
                Upstream::Direct | Upstream::WarpChain { .. } | Upstream::NoProxy,
                GatewayAttemptStatus::Failure,
            ) => {}
            (
                Upstream::Direct
                | Upstream::WarpChain { .. }
                | Upstream::NoProxy
                | Upstream::Warp { .. },
                GatewayAttemptStatus::Success | GatewayAttemptStatus::Unavailable,
            ) => {}
            (Upstream::Xray { .. } | Upstream::Proxy(_), GatewayAttemptStatus::Unavailable) => {}
        }
    }

    /// Select an upstream for the given host and protocol.
    pub async fn select(&self, host: &str, protocol: &str) -> Upstream {
        self.select_with_trace(host, protocol).await.upstream
    }

    /// Select an upstream and include a full route decision trace.
    pub async fn select_with_trace(&self, host: &str, protocol: &str) -> RouteSelection {
        let protocol = normalize_protocol(protocol);
        let plan = self.build_plan(host).await;
        let mut candidates = Vec::new();
        let mut upstream_candidates = Vec::new();
        let mut selected = None;

        for exit in plan.exits.iter().copied() {
            let resolved = self.resolve_exit(exit, &protocol, host).await;
            let source = plan.matched_reason.clone();
            match resolved {
                ResolvedExit::Available { upstreams } => {
                    for resolved_upstream in upstreams {
                        if selected.is_none() {
                            selected = Some(exit);
                        }
                        upstream_candidates.push(RouteUpstreamCandidate {
                            exit,
                            upstream: resolved_upstream.upstream,
                            detail: resolved_upstream.detail.clone(),
                        });
                        candidates.push(RouteCandidate {
                            exit,
                            priority: candidates.len(),
                            source: source.clone(),
                            available: true,
                            reason: None,
                            detail: resolved_upstream.detail,
                        });
                    }
                }
                ResolvedExit::Unavailable { reason } => {
                    candidates.push(RouteCandidate {
                        exit,
                        priority: candidates.len(),
                        source,
                        available: false,
                        reason: Some(reason),
                        detail: None,
                    });
                }
            }
        }

        let selected = selected.unwrap_or(RouteExit::NoProxy);
        if upstream_candidates.is_empty() {
            upstream_candidates.push(RouteUpstreamCandidate {
                exit: RouteExit::NoProxy,
                upstream: Upstream::NoProxy,
                detail: None,
            });
        }
        let upstream = upstream_candidates
            .first()
            .map(|candidate| candidate.upstream.clone())
            .unwrap_or(Upstream::NoProxy);
        let unavailable = candidates
            .iter()
            .filter_map(|candidate| {
                candidate.reason.as_ref().map(|reason| RouteUnavailable {
                    exit: candidate.exit,
                    reason: reason.clone(),
                })
            })
            .collect();

        RouteSelection {
            upstream,
            decision: RouteDecision {
                host: normalize_host(host),
                protocol,
                matched_group: plan.route_match.as_ref().map(|m| m.group.clone()),
                matched_rule: plan.route_match.as_ref().map(|m| m.matched_rule.clone()),
                tier: plan.tier.map(|t| t.as_str().to_string()),
                matched_reason: plan.matched_reason,
                geoip: plan.geoip,
                candidates,
                selected,
                unavailable,
            },
            upstream_candidates,
        }
    }

    /// Evaluate the route decision without opening a target tunnel.
    pub async fn dry_run(&self, host: &str, protocol: &str) -> RouteDecision {
        self.select_with_trace(host, protocol).await.decision
    }

    /// Render the selector's gateway metrics in Prometheus format.
    pub fn render_gateway_metrics(&self) -> String {
        self.metrics.render_prometheus()
    }

    async fn build_plan(&self, host: &str) -> RoutePlan {
        if let Some(router) = &self.router {
            // Router present: non-default → group policy; default → helpers then
            // group policy, optionally refined by GeoIP when default is not direct-only.
            let plan = route_match_plan(host, router.match_route(host), router);
            return self
                .maybe_refine_default_with_geoip(host, plan, router)
                .await;
        }

        if let Some(exits) = direct_reachable_domain_exits(host) {
            return RoutePlan {
                matched_reason: "direct_reachable_domain".into(),
                exits,
                route_match: None,
                geoip: None,
                tier: None,
            };
        }

        if let Some(exits) = business_domain_exits(host) {
            return RoutePlan {
                matched_reason: "business_domain_overseas".into(),
                exits,
                route_match: None,
                geoip: None,
                // Hardcoded business list uses premium-like exits (no free_pool).
                tier: Some(QualityTier::Premium),
            };
        }

        self.geoip_plan(host).await
    }

    /// When default group is not direct-only and GeoIP is configured, refine the
    /// plan: domestic → Direct; overseas/unknown → keep default-group exits.
    async fn maybe_refine_default_with_geoip(
        &self,
        host: &str,
        plan: RoutePlan,
        router: &Router,
    ) -> RoutePlan {
        if !should_refine_default_with_geoip(&plan, router) {
            return plan;
        }
        let Some(geoip) = &self.geoip else {
            return plan;
        };

        let (country, country_name, country_overseas) = {
            let mut geoip = geoip.lock().await;
            let info = geoip.lookup(host).await;
            let overseas = geoip.is_overseas(&info.country);
            (info.country, info.country_name, overseas)
        };

        refine_default_plan_with_geoip(plan, country, country_name, country_overseas)
    }

    async fn geoip_plan(&self, host: &str) -> RoutePlan {
        if let Some(geoip) = &self.geoip {
            let (geoip_decision, exits, reason) = {
                let mut geoip = geoip.lock().await;
                let info = geoip.lookup(host).await;
                let (overseas, reason) =
                    geoip_route_decision(&info.country, geoip.is_overseas(&info.country));
                let exits = geoip_exits(overseas);
                (
                    RouteGeoIpDecision {
                        country: info.country,
                        country_name: info.country_name,
                        overseas,
                    },
                    exits,
                    reason.to_string(),
                )
            };

            // Overseas geoip path matches premium order (xray → warp → no_proxy).
            let tier = geoip_decision.overseas.then_some(QualityTier::Premium);

            return RoutePlan {
                matched_reason: reason,
                exits,
                route_match: None,
                geoip: Some(geoip_decision),
                tier,
            };
        }

        RoutePlan {
            matched_reason: "general_fallback".into(),
            exits: general_fallback_exits(),
            route_match: None,
            geoip: None,
            // R3: no routes / general fallback = any.
            tier: Some(QualityTier::Any),
        }
    }

    async fn resolve_exit(&self, exit: RouteExit, protocol: &str, host: &str) -> ResolvedExit {
        match exit {
            RouteExit::Direct => ResolvedExit::Available {
                upstreams: vec![ResolvedUpstream {
                    upstream: Upstream::Direct,
                    detail: None,
                }],
            },
            RouteExit::FreePool => {
                let proxies = self
                    .try_pool_candidates(protocol, FREE_POOL_CANDIDATE_LIMIT, host)
                    .await;
                if proxies.is_empty() {
                    ResolvedExit::Unavailable {
                        reason: "no pool proxy available".into(),
                    }
                } else {
                    ResolvedExit::Available {
                        upstreams: proxies
                            .into_iter()
                            .map(|proxy| ResolvedUpstream {
                                detail: Some(proxy.dedup_key()),
                                upstream: Upstream::Proxy(proxy),
                            })
                            .collect(),
                    }
                }
            }
            RouteExit::Warp => match self.try_warp().await {
                Some(inst) => ResolvedExit::Available {
                    upstreams: vec![ResolvedUpstream {
                        upstream: Upstream::Warp {
                            id: inst.id,
                            socks5_host: inst.socks5_host.clone(),
                            socks5_port: inst.socks5_port,
                        },
                        detail: Some(format!("{}:{}", inst.socks5_host, inst.socks5_port)),
                    }],
                },
                None => ResolvedExit::Unavailable {
                    reason: "no healthy WARP instance available".into(),
                },
            },
            RouteExit::Xray => match self.try_xray().await {
                Some(port) => ResolvedExit::Available {
                    upstreams: vec![ResolvedUpstream {
                        upstream: Upstream::Xray {
                            local_socks5_port: port,
                        },
                        detail: Some(format!("127.0.0.1:{port}")),
                    }],
                },
                None => ResolvedExit::Unavailable {
                    reason: "no active xray node available".into(),
                },
            },
            RouteExit::NoProxy => ResolvedExit::Available {
                upstreams: vec![ResolvedUpstream {
                    upstream: Upstream::NoProxy,
                    detail: None,
                }],
            },
        }
    }

    /// Whether `host` should prefer proxies tagged for ChatGPT/OpenAI access.
    fn host_indicates_chatgpt(host: &str) -> bool {
        let h = host.to_ascii_lowercase();
        h.contains("openai.com") || h.contains("chatgpt.com")
    }

    async fn try_pool_candidates(&self, protocol: &str, limit: usize, host: &str) -> Vec<Proxy> {
        let proto = Protocol::from_str_loose(protocol).unwrap_or(Protocol::Http);
        let filtered = match self
            .store
            .get_top_candidates(proto, POOL_TOP_CANDIDATE_POOL, limit)
            .await
        {
            Ok(proxies) => {
                let failed_until = self.pool_proxy_failed_until.read().await;
                let now = Instant::now();
                proxies
                    .into_iter()
                    .filter(|proxy| {
                        !proxy.circuit_open
                            && proxy.encrypted_state.is_none()
                            && !pool_proxy_cooldown_active(&failed_until, &proxy.dedup_key(), now)
                    })
                    .collect::<Vec<_>>()
            }
            Err(e) => {
                tracing::debug!("try_pool_candidates: failed to query store: {e}");
                return Vec::new();
            }
        };

        // Prefer proxies tagged for ChatGPT/OpenAI when routing to those hosts.
        if Self::host_indicates_chatgpt(host)
            && let Ok(preferred_keys) = self
                .cap_store
                .get_proxies_with_tag(&CapabilityTag::ChatGPT)
                .await
        {
            let preferred: std::collections::HashSet<String> = preferred_keys.into_iter().collect();
            if !preferred.is_empty() {
                let (pref, rest): (Vec<_>, Vec<_>) = filtered
                    .into_iter()
                    .partition(|p| preferred.contains(&p.key()));
                let mut ordered = pref;
                ordered.extend(rest);
                return ordered.into_iter().take(limit).collect();
            }
        }

        filtered
    }

    async fn try_warp(&self) -> Option<WarpInstance> {
        if let Some(balancer) = &self.balancer
            && let Some(inst) = balancer.next().await
        {
            return Some(inst);
        }
        None
    }

    async fn try_xray(&self) -> Option<u16> {
        match self.store.all(Protocol::Socks5).await {
            Ok(proxies) => {
                let failed_until = self.xray_failed_until.read().await;
                let now = Instant::now();
                let mut active_xray: Vec<&Proxy> = proxies
                    .iter()
                    .filter(|p| {
                        xray_is_route_eligible(p) && !xray_cooldown_active(p, &failed_until, now)
                    })
                    .collect();
                if active_xray.is_empty() {
                    return None;
                }
                // Prefer lowest latency among eligible; random among ties.
                active_xray.sort_by(|a, b| {
                    let la = a.latency_ms.unwrap_or(f64::MAX);
                    let lb = b.latency_ms.unwrap_or(f64::MAX);
                    la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal)
                });
                let best_latency = active_xray[0].latency_ms.unwrap_or(f64::MAX);
                let tie_count = active_xray
                    .iter()
                    .take_while(|p| p.latency_ms.unwrap_or(f64::MAX) == best_latency)
                    .count()
                    .max(1);
                let idx = rand::random_range(0..tie_count);
                if let Some(EncryptedProxyState::Active { local_socks5_port }) =
                    active_xray[idx].encrypted_state
                {
                    Some(local_socks5_port)
                } else {
                    None
                }
            }
            Err(e) => {
                tracing::debug!("try_xray: failed to query store: {e}");
                None
            }
        }
    }
}

struct RoutePlan {
    matched_reason: String,
    exits: Vec<RouteExit>,
    route_match: Option<RouteMatch>,
    geoip: Option<RouteGeoIpDecision>,
    /// Resolved quality tier for diagnostics; stringified only at the API boundary.
    tier: Option<QualityTier>,
}

enum ResolvedExit {
    Available { upstreams: Vec<ResolvedUpstream> },
    Unavailable { reason: String },
}

struct ResolvedUpstream {
    upstream: Upstream,
    detail: Option<String>,
}

/// Exit order tables for quality tiers (D1/D6).
pub fn exits_for_tier(tier: QualityTier) -> Vec<RouteExit> {
    match tier {
        QualityTier::Any => vec![
            RouteExit::FreePool,
            RouteExit::Warp,
            RouteExit::Xray,
            RouteExit::NoProxy,
        ],
        QualityTier::Standard => vec![
            RouteExit::Xray,
            RouteExit::Warp,
            RouteExit::FreePool,
            RouteExit::NoProxy,
        ],
        QualityTier::Premium => vec![RouteExit::Xray, RouteExit::Warp, RouteExit::NoProxy],
    }
}

/// Resolve exits + tier diagnostics for a configured route group.
fn resolve_group_policy(router: &Router, group: &str) -> (Vec<RouteExit>, Option<QualityTier>) {
    if let Some(names) = router.exit_override_for(group) {
        let exits: Vec<RouteExit> = names
            .iter()
            .filter_map(|n| RouteExit::from_name(n))
            .collect();
        return (exits, router.tier_for(group));
    }

    match router.tier_for(group) {
        Some(tier) => (exits_for_tier(tier), Some(tier)),
        // No tier and no override → Direct-only (typically group `direct`).
        // `is_direct_only` is exactly this case once overrides are ruled out above.
        None => (vec![RouteExit::Direct], None),
    }
}

fn general_fallback_exits() -> Vec<RouteExit> {
    exits_for_tier(QualityTier::Any)
}

fn geoip_exits(overseas: bool) -> Vec<RouteExit> {
    if overseas {
        // Overseas without router: premium-like (xray → warp → no_proxy).
        exits_for_tier(QualityTier::Premium)
    } else {
        vec![RouteExit::Direct]
    }
}

fn route_match_plan(host: &str, route_match: RouteMatch, router: &Router) -> RoutePlan {
    if !route_match.is_default {
        // Non-default match: tier/group policy wins over BUSINESS_OVERSEAS hardcode.
        let (exits, tier) = resolve_group_policy(router, &route_match.group);
        return RoutePlan {
            matched_reason: "route_rule".into(),
            exits,
            route_match: Some(route_match),
            geoip: None,
            tier,
        };
    }

    // Default match: keep domain helpers first, then default-group policy.
    // GeoIP refinement (when available and default group is not direct-only) is
    // applied later in `UpstreamSelector::maybe_refine_default_with_geoip`.
    if let Some(exits) = direct_reachable_domain_exits(host) {
        return RoutePlan {
            matched_reason: "direct_reachable_domain".into(),
            exits,
            route_match: Some(route_match),
            geoip: None,
            tier: None,
        };
    }

    if let Some(exits) = business_domain_exits(host) {
        return RoutePlan {
            matched_reason: "business_domain_overseas".into(),
            exits,
            route_match: Some(route_match),
            geoip: None,
            tier: Some(QualityTier::Premium),
        };
    }

    let (exits, tier) = resolve_group_policy(router, &route_match.group);
    RoutePlan {
        matched_reason: "route_default_group".into(),
        exits,
        route_match: Some(route_match),
        geoip: None,
        tier,
    }
}

/// Whether `build_plan` should consult GeoIP for a default-group plan.
///
/// Requires `route_default_group` (helpers already skipped) and a non direct-only
/// default group. domestic-friendly (`default` under `direct`) returns false so
/// GeoIP cannot rewrite the plan to overseas exits.
fn should_refine_default_with_geoip(plan: &RoutePlan, router: &Router) -> bool {
    if plan.matched_reason != "route_default_group" {
        return false;
    }
    plan.route_match
        .as_ref()
        .is_some_and(|m| !router.is_direct_only(&m.group))
}

/// Refine a default-group plan with GeoIP when the group is not direct-only.
///
/// Callers must already filter helpers and direct-only defaults. Domestic → Direct;
/// overseas / UNKNOWN → keep the default group's exits and tier.
fn refine_default_plan_with_geoip(
    plan: RoutePlan,
    country: String,
    country_name: String,
    country_overseas: bool,
) -> RoutePlan {
    let (overseas, reason) = geoip_route_decision(&country, country_overseas);
    let geoip = Some(RouteGeoIpDecision {
        country,
        country_name,
        overseas,
    });

    if overseas {
        RoutePlan {
            matched_reason: reason.into(),
            exits: plan.exits,
            route_match: plan.route_match,
            geoip,
            tier: plan.tier,
        }
    } else {
        RoutePlan {
            matched_reason: reason.into(),
            exits: geoip_exits(false),
            route_match: plan.route_match,
            geoip,
            tier: None,
        }
    }
}

fn geoip_route_decision(country: &str, country_overseas: bool) -> (bool, &'static str) {
    if country == "UNKNOWN" {
        (true, "geoip_unknown_overseas")
    } else if country_overseas {
        (true, "geoip_overseas")
    } else {
        (false, "geoip_domestic")
    }
}

fn direct_reachable_domain_exits(host: &str) -> Option<Vec<RouteExit>> {
    if is_direct_reachable_host(host) {
        Some(vec![RouteExit::Direct])
    } else {
        None
    }
}

fn is_direct_reachable_host(host: &str) -> bool {
    domain_list_matches(DIRECT_REACHABLE_DOMAINS, host)
}

fn business_domain_exits(host: &str) -> Option<Vec<RouteExit>> {
    if is_business_overseas_host(host) {
        Some(geoip_exits(true))
    } else {
        None
    }
}

fn is_business_overseas_host(host: &str) -> bool {
    domain_list_matches(BUSINESS_OVERSEAS_DOMAINS, host)
}

fn domain_list_matches(domains: &[&str], host: &str) -> bool {
    let host = normalize_host(host);
    domains
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

fn normalize_protocol(protocol: &str) -> String {
    Protocol::from_str_loose(protocol)
        .unwrap_or(Protocol::Http)
        .to_string()
}

fn normalize_host(host: &str) -> String {
    let host = host.trim().to_lowercase();
    let host = host.split(':').next().unwrap_or(&host);
    host.trim_end_matches('.').to_string()
}

fn pool_proxy_cooldown_active(
    cooldowns: &HashMap<String, Instant>,
    key: &str,
    now: Instant,
) -> bool {
    matches!(cooldowns.get(key), Some(until) if *until > now)
}

/// Age in seconds of the latest successful evidence, if any.
///
/// Preference order:
/// 1. Latest successful `quality_history` sample timestamp
/// 2. `last_check` when `success_count > 0`
fn xray_fresh_success_age_secs(proxy: &Proxy, now: DateTime<Utc>) -> Option<i64> {
    if let Some(sample) = proxy
        .quality_history
        .samples
        .iter()
        .rev()
        .find(|sample| sample.success)
    {
        return Some((now.timestamp() - sample.checked_at_unix_secs).max(0));
    }
    if proxy.success_count > 0 {
        return proxy
            .last_check
            .map(|checked| (now.timestamp() - checked.timestamp()).max(0));
    }
    None
}

/// Whether an xray pool entry has fresh success evidence for routing.
///
/// Requires `success_count > 0` and latest successful evidence within
/// [`XRAY_ROUTE_FRESH_SUCCESS_SECS`] (Decision D2: 15m). Circuit-open and
/// Active-state checks live in [`xray_is_route_eligible`].
fn xray_has_validation_evidence(proxy: &Proxy) -> bool {
    if proxy.success_count == 0 {
        return false;
    }
    let now = Utc::now();
    match xray_fresh_success_age_secs(proxy, now) {
        Some(age) => age <= XRAY_ROUTE_FRESH_SUCCESS_SECS,
        None => false,
    }
}

/// Full route eligibility for an xray pool entry (excluding gateway cooldown).
fn xray_is_route_eligible(proxy: &Proxy) -> bool {
    matches!(
        proxy.encrypted_state,
        Some(EncryptedProxyState::Active { .. })
    ) && !circuit::is_circuit_open(proxy)
        && xray_has_validation_evidence(proxy)
}

/// Pick the lowest-latency eligible xray proxy; random among equal latency ties.
///
/// Exposed for unit tests of selection preference.
#[cfg(test)]
fn select_lowest_latency_xray<'a>(candidates: &[&'a Proxy]) -> Option<&'a Proxy> {
    if candidates.is_empty() {
        return None;
    }
    let mut ordered: Vec<&Proxy> = candidates.to_vec();
    ordered.sort_by(|a, b| {
        let la = a.latency_ms.unwrap_or(f64::MAX);
        let lb = b.latency_ms.unwrap_or(f64::MAX);
        la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal)
    });
    Some(ordered[0])
}

fn xray_cooldown_active(proxy: &Proxy, cooldowns: &HashMap<u16, Instant>, now: Instant) -> bool {
    match proxy.encrypted_state {
        Some(EncryptedProxyState::Active { local_socks5_port }) => {
            matches!(cooldowns.get(&local_socks5_port), Some(until) if *until > now)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::Router;
    use std::collections::HashMap;

    fn test_router() -> Router {
        let mut groups = HashMap::new();
        groups.insert("direct".into(), vec!["*.cn".into(), "default".into()]);
        groups.insert("free_pool".into(), vec!["github.com".into()]);
        groups.insert("warp".into(), vec!["cloudflare.com".into()]);
        groups.insert("xray".into(), vec!["xray.test".into()]);
        groups.insert("custom".into(), vec!["custom.example".into()]);
        groups.insert(
            "openai".into(),
            vec!["openai.com".into(), "chatgpt.com".into()],
        );
        // openai gets default custom tier=any unless we load extended YAML.
        Router::new(groups).unwrap()
    }

    fn premium_router() -> Router {
        Router::from_yaml_str(
            r#"
groups:
  direct:
    domains: ["*.cn", default]
  free_pool:
    tier: any
    domains: ["github.com"]
  openai:
    tier: premium
    domains: ["openai.com", "chatgpt.com"]
  standard_sites:
    tier: standard
    domains: ["example-std.com"]
"#,
        )
        .unwrap()
    }

    #[test]
    fn exits_for_tier_tables_match_d6() {
        assert_eq!(
            exits_for_tier(QualityTier::Any),
            vec![
                RouteExit::FreePool,
                RouteExit::Warp,
                RouteExit::Xray,
                RouteExit::NoProxy
            ]
        );
        assert_eq!(
            exits_for_tier(QualityTier::Standard),
            vec![
                RouteExit::Xray,
                RouteExit::Warp,
                RouteExit::FreePool,
                RouteExit::NoProxy
            ]
        );
        assert_eq!(
            exits_for_tier(QualityTier::Premium),
            vec![RouteExit::Xray, RouteExit::Warp, RouteExit::NoProxy]
        );
        assert!(!exits_for_tier(QualityTier::Premium).contains(&RouteExit::FreePool));
    }

    #[test]
    fn known_group_candidate_orders_match_runtime_contract() {
        let router = test_router();
        assert_eq!(
            resolve_group_policy(&router, "direct"),
            (vec![RouteExit::Direct], None)
        );
        assert_eq!(
            resolve_group_policy(&router, "free_pool"),
            (exits_for_tier(QualityTier::Any), Some(QualityTier::Any))
        );
        // warp/xray both map to premium (xray → warp → no_proxy).
        assert_eq!(
            resolve_group_policy(&router, "warp"),
            (
                exits_for_tier(QualityTier::Premium),
                Some(QualityTier::Premium)
            )
        );
        assert_eq!(
            resolve_group_policy(&router, "xray"),
            (
                exits_for_tier(QualityTier::Premium),
                Some(QualityTier::Premium)
            )
        );
        // Custom groups without explicit tier default to any.
        assert_eq!(
            resolve_group_policy(&router, "custom"),
            (exits_for_tier(QualityTier::Any), Some(QualityTier::Any))
        );
    }

    #[test]
    fn geoip_candidate_orders_match_runtime_contract() {
        assert_eq!(geoip_exits(false), vec![RouteExit::Direct]);
        assert_eq!(
            geoip_exits(true),
            vec![RouteExit::Xray, RouteExit::Warp, RouteExit::NoProxy]
        );
    }

    #[test]
    fn geoip_unknown_routes_as_overseas_for_gateway_planning() {
        assert_eq!(
            geoip_route_decision("UNKNOWN", false),
            (true, "geoip_unknown_overseas")
        );
        assert_eq!(geoip_route_decision("CN", false), (false, "geoip_domestic"));
        assert_eq!(geoip_route_decision("US", true), (true, "geoip_overseas"));
    }

    #[test]
    fn business_domains_match_roots_and_subdomains_only() {
        assert!(is_business_overseas_host("openai.com"));
        assert!(is_business_overseas_host("api.openai.com:443"));
        assert!(is_business_overseas_host("WWW.REDDIT.COM."));
        assert!(!is_business_overseas_host("notopenai.com"));
        assert!(!is_business_overseas_host("openai.com.example"));
        assert!(!is_business_overseas_host("github.com"));
        assert!(!is_business_overseas_host("news.ycombinator.com"));
    }

    #[test]
    fn direct_reachable_domains_match_roots_and_subdomains_only() {
        assert!(is_direct_reachable_host("github.com"));
        assert!(is_direct_reachable_host("api.github.com:443"));
        assert!(is_direct_reachable_host("NEWS.YCOMBINATOR.COM."));
        assert!(!is_direct_reachable_host("notgithub.com"));
        assert!(!is_direct_reachable_host("github.com.example"));
        assert_eq!(
            direct_reachable_domain_exits("news.ycombinator.com"),
            Some(vec![RouteExit::Direct])
        );
    }

    #[test]
    fn business_domain_exits_use_overseas_candidate_order() {
        assert_eq!(
            business_domain_exits("chatgpt.com"),
            Some(geoip_exits(true))
        );
        assert_eq!(business_domain_exits("example.com"), None);
    }

    #[test]
    fn router_default_does_not_mask_business_domain() {
        let router = test_router();
        let plan = route_match_plan(
            "api.openai.com",
            RouteMatch {
                group: "direct".into(),
                matched_rule: "default".into(),
                is_default: true,
            },
            &router,
        );

        assert_eq!(plan.matched_reason, "business_domain_overseas");
        assert_eq!(plan.exits, geoip_exits(true));
        assert_eq!(plan.tier, Some(QualityTier::Premium));
        let route_match = plan.route_match.unwrap();
        assert_eq!(route_match.group, "direct");
        assert!(route_match.is_default);
    }

    #[test]
    fn direct_reachable_domain_wins_before_default_and_geoip_fallback() {
        let router = test_router();
        let plan = route_match_plan(
            "github.com",
            RouteMatch {
                group: "warp".into(),
                matched_rule: "default".into(),
                is_default: true,
            },
            &router,
        );

        assert_eq!(plan.matched_reason, "direct_reachable_domain");
        assert_eq!(plan.exits, vec![RouteExit::Direct]);
        assert!(plan.route_match.unwrap().is_default);
    }

    fn overseas_stable_router() -> Router {
        Router::from_yaml_str(
            r#"
groups:
  direct:
    domains: ["*.cn"]
  overseas:
    tier: premium
    domains: [default]
  free_pool:
    tier: any
    domains: ["github.com"]
"#,
        )
        .unwrap()
    }

    fn default_premium_plan(router: &Router) -> RoutePlan {
        route_match_plan(
            "unlisted.example",
            router.match_route("unlisted.example"),
            router,
        )
    }

    #[test]
    fn routes_example_yaml_is_overseas_stable() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/routes.example.yaml");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let router = Router::from_yaml_str(&text).expect("routes.example.yaml must parse");

        let unknown = router.match_route("unknown.example");
        assert!(unknown.is_default);
        assert_eq!(unknown.group, "overseas");
        assert_eq!(
            router.tier_for(&unknown.group),
            Some(QualityTier::Premium),
            "default group must be premium (overseas-stable)"
        );
        assert!(!router.is_direct_only(&unknown.group));

        assert_eq!(router.match_route("foo.cn").group, "direct");
        assert!(router.is_direct_only("direct"));

        // free_pool must not own default; premium exits never include free_pool.
        let default_plan = route_match_plan("unknown.example", unknown, &router);
        assert_eq!(default_plan.tier, Some(QualityTier::Premium));
        assert!(!default_plan.exits.contains(&RouteExit::FreePool));
    }

    #[test]
    fn default_premium_geoip_domestic_goes_direct() {
        let router = overseas_stable_router();
        let plan = default_premium_plan(&router);
        assert_eq!(plan.matched_reason, "route_default_group");
        assert_eq!(plan.tier, Some(QualityTier::Premium));

        let refined =
            refine_default_plan_with_geoip(plan, "CN".into(), "China".into(), false);
        assert_eq!(refined.matched_reason, "geoip_domestic");
        assert_eq!(refined.exits, vec![RouteExit::Direct]);
        assert_eq!(refined.tier, None);
        assert!(refined.geoip.as_ref().is_some_and(|g| !g.overseas));
        assert!(refined.route_match.as_ref().is_some_and(|m| m.is_default));
    }

    #[test]
    fn default_premium_geoip_overseas_keeps_group_exits() {
        let router = overseas_stable_router();
        let plan = default_premium_plan(&router);
        let refined = refine_default_plan_with_geoip(
            plan,
            "US".into(),
            "United States".into(),
            true,
        );
        assert_eq!(refined.matched_reason, "geoip_overseas");
        assert_eq!(refined.exits, exits_for_tier(QualityTier::Premium));
        assert_eq!(refined.tier, Some(QualityTier::Premium));
        assert!(refined.geoip.as_ref().is_some_and(|g| g.overseas));
    }

    #[test]
    fn default_premium_geoip_unknown_treated_as_overseas() {
        let router = overseas_stable_router();
        let plan = default_premium_plan(&router);
        let refined =
            refine_default_plan_with_geoip(plan, "UNKNOWN".into(), "Unknown".into(), false);
        assert_eq!(refined.matched_reason, "geoip_unknown_overseas");
        assert_eq!(refined.exits, exits_for_tier(QualityTier::Premium));
        assert_eq!(refined.tier, Some(QualityTier::Premium));
    }

    #[test]
    fn domestic_friendly_default_direct_only_not_rewritten_by_geoip_filter() {
        // domestic-friendly: default lives in direct (direct-only).
        // Gate must skip GeoIP so overseas lookup cannot rewrite Direct exits.
        let router = test_router();
        let plan = route_match_plan(
            "unlisted.example",
            router.match_route("unlisted.example"),
            &router,
        );
        assert_eq!(plan.matched_reason, "route_default_group");
        assert_eq!(plan.exits, vec![RouteExit::Direct]);
        assert_eq!(plan.tier, None);
        assert!(router.is_direct_only(&plan.route_match.as_ref().unwrap().group));
        assert!(
            !should_refine_default_with_geoip(&plan, &router),
            "direct-only default must not enter GeoIP refine"
        );
    }

    #[test]
    fn non_default_route_rule_not_subject_to_default_geoip_reason() {
        let router = overseas_stable_router();
        let plan = route_match_plan("foo.cn", router.match_route("foo.cn"), &router);
        assert_eq!(plan.matched_reason, "route_rule");
        assert_eq!(plan.exits, vec![RouteExit::Direct]);
        assert!(!should_refine_default_with_geoip(&plan, &router));
    }

    #[test]
    fn overseas_stable_default_allows_geoip_refine_gate() {
        let router = overseas_stable_router();
        let plan = default_premium_plan(&router);
        assert!(should_refine_default_with_geoip(&plan, &router));
    }

    #[test]
    fn explicit_route_rule_wins_over_business_domain_fallback() {
        let router = test_router();
        let plan = route_match_plan(
            "api.openai.com",
            RouteMatch {
                group: "direct".into(),
                matched_rule: "openai.com".into(),
                is_default: false,
            },
            &router,
        );

        assert_eq!(plan.matched_reason, "route_rule");
        assert_eq!(plan.exits, vec![RouteExit::Direct]);
        assert_eq!(plan.tier, None);
        assert!(!plan.route_match.unwrap().is_default);

        // Custom groups default to tier=any (no longer fall through to geoip).
        let custom_plan = route_match_plan(
            "api.openai.com",
            RouteMatch {
                group: "custom".into(),
                matched_rule: "openai.com".into(),
                is_default: false,
            },
            &router,
        );
        assert_eq!(custom_plan.matched_reason, "route_rule");
        assert_eq!(custom_plan.exits, exits_for_tier(QualityTier::Any));
        assert_eq!(custom_plan.tier, Some(QualityTier::Any));
    }

    #[test]
    fn tiered_route_plans_follow_quality_tables() {
        let router = premium_router();

        let any_plan = route_match_plan("github.com", router.match_route("github.com"), &router);
        assert_eq!(any_plan.tier, Some(QualityTier::Any));
        assert_eq!(any_plan.exits, exits_for_tier(QualityTier::Any));
        assert!(any_plan.exits.contains(&RouteExit::FreePool));

        let premium_plan = route_match_plan(
            "api.openai.com",
            router.match_route("api.openai.com"),
            &router,
        );
        assert_eq!(premium_plan.tier, Some(QualityTier::Premium));
        assert_eq!(premium_plan.exits, exits_for_tier(QualityTier::Premium));
        assert!(!premium_plan.exits.contains(&RouteExit::FreePool));
        assert_eq!(premium_plan.exits.last().copied(), Some(RouteExit::NoProxy));

        let chatgpt_plan =
            route_match_plan("chatgpt.com", router.match_route("chatgpt.com"), &router);
        assert_eq!(chatgpt_plan.tier, Some(QualityTier::Premium));
        assert!(!chatgpt_plan.exits.contains(&RouteExit::FreePool));

        let standard_plan = route_match_plan(
            "example-std.com",
            router.match_route("example-std.com"),
            &router,
        );
        assert_eq!(standard_plan.tier, Some(QualityTier::Standard));
        assert_eq!(standard_plan.exits, exits_for_tier(QualityTier::Standard));
    }

    #[test]
    fn exit_override_replaces_tier_table_in_plan() {
        let router = Router::from_yaml_str(
            r#"
groups:
  direct:
    domains: [default]
  custom:
    tier: standard
    domains: ["override.example"]
    exits: [warp, xray, no_proxy]
"#,
        )
        .unwrap();

        let plan = route_match_plan(
            "override.example",
            router.match_route("override.example"),
            &router,
        );
        assert_eq!(plan.matched_reason, "route_rule");
        assert_eq!(plan.tier, Some(QualityTier::Standard));
        // Override wins over standard table (which would put FreePool after Warp).
        assert_eq!(
            plan.exits,
            vec![RouteExit::Warp, RouteExit::Xray, RouteExit::NoProxy]
        );
        assert!(!plan.exits.contains(&RouteExit::FreePool));
    }

    #[test]
    fn general_fallback_reaches_no_proxy() {
        assert_eq!(
            general_fallback_exits(),
            vec![
                RouteExit::FreePool,
                RouteExit::Warp,
                RouteExit::Xray,
                RouteExit::NoProxy
            ]
        );
    }

    #[test]
    fn route_decision_serializes_stable_exit_names() {
        let decision = RouteDecision {
            host: "example.com".into(),
            protocol: "http".into(),
            matched_group: Some("free_pool".into()),
            matched_rule: Some("example.com".into()),
            tier: Some("any".into()),
            matched_reason: "route_rule".into(),
            geoip: None,
            candidates: vec![RouteCandidate {
                exit: RouteExit::FreePool,
                priority: 0,
                source: "route_rule".into(),
                available: false,
                reason: Some("no pool proxy available".into()),
                detail: None,
            }],
            selected: RouteExit::NoProxy,
            unavailable: vec![RouteUnavailable {
                exit: RouteExit::FreePool,
                reason: "no pool proxy available".into(),
            }],
        };

        let json = serde_json::to_string(&decision).unwrap();
        assert!(json.contains("\"free_pool\""));
        assert!(json.contains("\"no_proxy\""));
        assert!(json.contains("\"matched_group\":\"free_pool\""));
        assert!(json.contains("\"tier\":\"any\""));
    }

    #[test]
    fn gateway_metrics_render_all_label_dimensions() {
        let metrics = GatewayRouteMetrics::new();
        metrics.record(
            GatewayRouteProtocol::HttpConnect,
            RouteExit::Warp,
            GatewayAttemptStatus::Failure,
        );

        let rendered = metrics.render_prometheus();
        assert!(rendered.contains("proxy_gateway_route_attempts_total"));
        assert!(rendered.contains("protocol=\"http_connect\",exit=\"warp\",status=\"failure\"} 1"));
        assert!(
            rendered.contains("protocol=\"socks5\",exit=\"no_proxy\",status=\"unavailable\"} 0")
        );
    }

    #[test]
    fn gateway_metrics_fixed_series_are_closed_and_complete() {
        let rendered = GatewayRouteMetrics::new().render_prometheus();

        let mut expected = 0usize;
        for protocol in GatewayRouteProtocol::ALL {
            for exit in RouteExit::ALL {
                for status in GatewayAttemptStatus::ALL {
                    let needle = format!(
                        "proxy_gateway_route_attempts_total{{protocol=\"{}\",exit=\"{}\",status=\"{}\"}}",
                        protocol.label(),
                        exit.label(),
                        status.label()
                    );
                    assert!(
                        rendered.contains(&needle),
                        "missing fixed gateway series: {needle}"
                    );
                    expected += 1;
                }
            }
        }

        let series_count = rendered
            .lines()
            .filter(|line| line.starts_with("proxy_gateway_route_attempts_total{"))
            .count();
        assert_eq!(series_count, expected);
        assert_eq!(
            series_count,
            GatewayRouteProtocol::ALL.len()
                * RouteExit::ALL.len()
                * GatewayAttemptStatus::ALL.len()
        );
        // Freeze the Prometheus contract cardinality for scrapers.
        assert_eq!(series_count, 45);
        assert_eq!(METRIC_CELL_COUNT, 45);
    }

    #[test]
    fn pool_proxy_cooldown_active_only_before_deadline() {
        let now = Instant::now();
        let mut cooldowns = HashMap::new();
        cooldowns.insert(
            "http:1.2.3.4:8080".to_string(),
            now + Duration::from_secs(60),
        );
        cooldowns.insert(
            "http:5.6.7.8:8080".to_string(),
            now - Duration::from_secs(1),
        );

        assert!(pool_proxy_cooldown_active(
            &cooldowns,
            "http:1.2.3.4:8080",
            now
        ));
        assert!(!pool_proxy_cooldown_active(
            &cooldowns,
            "http:5.6.7.8:8080",
            now
        ));
        assert!(!pool_proxy_cooldown_active(
            &cooldowns,
            "http:9.9.9.9:8080",
            now
        ));
    }

    #[test]
    fn xray_validation_evidence_requires_successful_check() {
        let mut proxy = Proxy::new("127.0.0.1", 20000, Protocol::Socks5);
        proxy.encrypted_state = Some(EncryptedProxyState::Active {
            local_socks5_port: 20000,
        });

        assert!(!xray_has_validation_evidence(&proxy));

        proxy.last_check = Some(chrono::Utc::now());
        assert!(!xray_has_validation_evidence(&proxy));

        proxy.success_count = 1;
        assert!(xray_has_validation_evidence(&proxy));
    }

    #[test]
    fn xray_validation_evidence_rejects_stale_success() {
        let mut proxy = Proxy::new("127.0.0.1", 20000, Protocol::Socks5);
        proxy.encrypted_state = Some(EncryptedProxyState::Active {
            local_socks5_port: 20000,
        });
        proxy.success_count = 3;
        proxy.last_check =
            Some(Utc::now() - chrono::Duration::seconds(XRAY_ROUTE_FRESH_SUCCESS_SECS + 1));

        assert!(!xray_has_validation_evidence(&proxy));
        assert!(!xray_is_route_eligible(&proxy));
    }

    #[test]
    fn xray_validation_evidence_accepts_fresh_quality_history_success() {
        let mut proxy = Proxy::new("127.0.0.1", 20000, Protocol::Socks5);
        proxy.encrypted_state = Some(EncryptedProxyState::Active {
            local_socks5_port: 20000,
        });
        proxy.success_count = 2;
        // last_check is stale, but quality_history has a fresh success.
        proxy.last_check =
            Some(Utc::now() - chrono::Duration::seconds(XRAY_ROUTE_FRESH_SUCCESS_SECS + 60));
        proxy
            .quality_history
            .record_success(Utc::now() - chrono::Duration::seconds(30), Some(120.0));

        assert!(xray_has_validation_evidence(&proxy));
        assert!(xray_is_route_eligible(&proxy));
    }

    #[test]
    fn xray_route_eligibility_rejects_circuit_open() {
        let mut proxy = Proxy::new("127.0.0.1", 20000, Protocol::Socks5);
        proxy.encrypted_state = Some(EncryptedProxyState::Active {
            local_socks5_port: 20000,
        });
        proxy.success_count = 5;
        proxy.last_check = Some(Utc::now());
        proxy.latency_ms = Some(80.0);
        proxy.circuit_open = true;
        proxy.circuit_open_until = Some(Utc::now() + chrono::Duration::seconds(600));

        assert!(xray_has_validation_evidence(&proxy));
        assert!(!xray_is_route_eligible(&proxy));
    }

    #[test]
    fn xray_selection_prefers_lowest_latency_among_eligible() {
        let mut slow = Proxy::new("127.0.0.1", 20001, Protocol::Socks5);
        slow.encrypted_state = Some(EncryptedProxyState::Active {
            local_socks5_port: 20001,
        });
        slow.success_count = 1;
        slow.last_check = Some(Utc::now());
        slow.latency_ms = Some(400.0);

        let mut fast = Proxy::new("127.0.0.1", 20000, Protocol::Socks5);
        fast.encrypted_state = Some(EncryptedProxyState::Active {
            local_socks5_port: 20000,
        });
        fast.success_count = 1;
        fast.last_check = Some(Utc::now());
        fast.latency_ms = Some(90.0);

        let mut stale = Proxy::new("127.0.0.1", 20002, Protocol::Socks5);
        stale.encrypted_state = Some(EncryptedProxyState::Active {
            local_socks5_port: 20002,
        });
        stale.success_count = 1;
        stale.last_check =
            Some(Utc::now() - chrono::Duration::seconds(XRAY_ROUTE_FRESH_SUCCESS_SECS + 10));
        stale.latency_ms = Some(10.0);

        let candidates: Vec<&Proxy> = [&slow, &fast, &stale]
            .into_iter()
            .filter(|p| xray_is_route_eligible(p))
            .collect();
        assert_eq!(candidates.len(), 2);
        let chosen = select_lowest_latency_xray(&candidates).unwrap();
        assert_eq!(chosen.port, 20000);
    }

    #[test]
    fn xray_selection_empty_when_only_stale_or_open() {
        let mut stale = Proxy::new("127.0.0.1", 20000, Protocol::Socks5);
        stale.encrypted_state = Some(EncryptedProxyState::Active {
            local_socks5_port: 20000,
        });
        stale.success_count = 2;
        stale.last_check =
            Some(Utc::now() - chrono::Duration::seconds(XRAY_ROUTE_FRESH_SUCCESS_SECS + 5));

        let mut open = Proxy::new("127.0.0.1", 20001, Protocol::Socks5);
        open.encrypted_state = Some(EncryptedProxyState::Active {
            local_socks5_port: 20001,
        });
        open.success_count = 2;
        open.last_check = Some(Utc::now());
        open.circuit_open = true;
        open.circuit_open_until = Some(Utc::now() + chrono::Duration::seconds(300));

        let candidates: Vec<&Proxy> = [&stale, &open]
            .into_iter()
            .filter(|p| xray_is_route_eligible(p))
            .collect();
        assert!(candidates.is_empty());
        assert!(select_lowest_latency_xray(&candidates).is_none());
    }

    #[test]
    fn xray_cooldown_active_only_before_deadline() {
        let now = Instant::now();
        let mut proxy = Proxy::new("127.0.0.1", 20000, Protocol::Socks5);
        proxy.encrypted_state = Some(EncryptedProxyState::Active {
            local_socks5_port: 20000,
        });
        let mut cooldowns = HashMap::new();
        cooldowns.insert(20000, now + Duration::from_secs(60));

        assert!(xray_cooldown_active(&proxy, &cooldowns, now));

        cooldowns.insert(20000, now - Duration::from_secs(1));
        assert!(!xray_cooldown_active(&proxy, &cooldowns, now));

        proxy.encrypted_state = None;
        assert!(!xray_cooldown_active(&proxy, &cooldowns, now));
    }
}
