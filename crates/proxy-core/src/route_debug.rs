//! Traceable gateway route selection and route diagnostics.

use crate::geoip::GeoIPLookup;
use crate::models::{EncryptedProxyState, Protocol, Proxy, WarpInstance};
use crate::router::{RouteMatch, Router};
use crate::store::ProxyStore;
use crate::warp::balancer::WarpBalancer;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;

/// Runtime upstream selected for a gateway request.
#[derive(Debug, Clone)]
pub enum Upstream {
    /// Connect directly to the target.
    Direct,
    /// Route through a pool proxy.
    Proxy(Proxy),
    /// Route through a WARP instance.
    Warp { id: u32, socks5_port: u16 },
    /// Route through an xray-node local SOCKS5 port.
    Xray { local_socks5_port: u16 },
    /// Chain through a pool proxy and then WARP.
    WarpChain { proxy: Proxy, socks5_port: u16 },
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
const FREE_POOL_CANDIDATE_LIMIT: usize = 4;

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
        Self {
            store,
            balancer,
            router,
            geoip,
            metrics,
        }
    }

    /// Return the shared gateway metrics registry.
    pub fn metrics(&self) -> Arc<GatewayRouteMetrics> {
        self.metrics.clone()
    }

    /// Feed concrete gateway attempt outcomes back into route health.
    pub async fn record_upstream_attempt(&self, upstream: &Upstream, status: GatewayAttemptStatus) {
        if status != GatewayAttemptStatus::Failure {
            return;
        }

        if let Upstream::Warp { id, socks5_port } = upstream
            && let Some(balancer) = &self.balancer
        {
            balancer.mark_failed(*id).await;
            tracing::warn!(
                warp_id = *id,
                socks5_port = *socks5_port,
                "gateway marked WARP instance unhealthy after connection failure"
            );
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
            let resolved = self.resolve_exit(exit, &protocol).await;
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
            let route_match = router.match_route(host);
            if let Some(exits) = exits_for_known_group(&route_match.group) {
                return RoutePlan {
                    matched_reason: if route_match.is_default {
                        "route_default_group".into()
                    } else {
                        "route_rule".into()
                    },
                    exits,
                    route_match: Some(route_match),
                    geoip: None,
                };
            }

            let mut plan = self.geoip_plan(host).await;
            plan.route_match = Some(route_match);
            return plan;
        }

        self.geoip_plan(host).await
    }

    async fn geoip_plan(&self, host: &str) -> RoutePlan {
        if let Some(geoip) = &self.geoip {
            let (geoip_decision, exits, reason) = {
                let mut geoip = geoip.lock().await;
                let info = geoip.lookup(host).await;
                let overseas = geoip.is_overseas(&info.country);
                let exits = geoip_exits(overseas);
                let reason = if overseas {
                    "geoip_overseas"
                } else {
                    "geoip_domestic"
                };
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

            return RoutePlan {
                matched_reason: reason,
                exits,
                route_match: None,
                geoip: Some(geoip_decision),
            };
        }

        RoutePlan {
            matched_reason: "general_fallback".into(),
            exits: general_fallback_exits(),
            route_match: None,
            geoip: None,
        }
    }

    async fn resolve_exit(&self, exit: RouteExit, protocol: &str) -> ResolvedExit {
        match exit {
            RouteExit::Direct => ResolvedExit::Available {
                upstreams: vec![ResolvedUpstream {
                    upstream: Upstream::Direct,
                    detail: None,
                }],
            },
            RouteExit::FreePool => {
                let proxies = self
                    .try_pool_candidates(protocol, FREE_POOL_CANDIDATE_LIMIT)
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
                            socks5_port: inst.socks5_port,
                        },
                        detail: Some(format!("127.0.0.1:{}", inst.socks5_port)),
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

    async fn try_pool_candidates(&self, protocol: &str, limit: usize) -> Vec<Proxy> {
        let proto = Protocol::from_str_loose(protocol).unwrap_or(Protocol::Http);
        match self.store.get_random_candidates(proto, limit).await {
            Ok(proxies) => proxies
                .into_iter()
                .filter(|proxy| !proxy.circuit_open && proxy.encrypted_state.is_none())
                .collect(),
            Err(e) => {
                tracing::debug!("try_pool_candidates: failed to query store: {e}");
                Vec::new()
            }
        }
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
                let active_xray: Vec<&Proxy> = proxies
                    .iter()
                    .filter(|p| {
                        matches!(p.encrypted_state, Some(EncryptedProxyState::Active { .. }))
                    })
                    .collect();
                if active_xray.is_empty() {
                    return None;
                }
                let idx = rand::random_range(0..active_xray.len());
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
}

enum ResolvedExit {
    Available { upstreams: Vec<ResolvedUpstream> },
    Unavailable { reason: String },
}

struct ResolvedUpstream {
    upstream: Upstream,
    detail: Option<String>,
}

fn exits_for_known_group(group: &str) -> Option<Vec<RouteExit>> {
    match group {
        "direct" => Some(vec![RouteExit::Direct]),
        "free_pool" => Some(vec![
            RouteExit::FreePool,
            RouteExit::Warp,
            RouteExit::Xray,
            RouteExit::NoProxy,
        ]),
        "warp" => Some(vec![
            RouteExit::Warp,
            RouteExit::Xray,
            RouteExit::FreePool,
            RouteExit::NoProxy,
        ]),
        "xray" => Some(vec![
            RouteExit::Xray,
            RouteExit::FreePool,
            RouteExit::Warp,
            RouteExit::NoProxy,
        ]),
        _ => None,
    }
}

fn general_fallback_exits() -> Vec<RouteExit> {
    vec![
        RouteExit::FreePool,
        RouteExit::Warp,
        RouteExit::Xray,
        RouteExit::NoProxy,
    ]
}

fn geoip_exits(overseas: bool) -> Vec<RouteExit> {
    if overseas {
        vec![
            RouteExit::Warp,
            RouteExit::Xray,
            RouteExit::FreePool,
            RouteExit::NoProxy,
        ]
    } else {
        vec![RouteExit::Direct]
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_group_candidate_orders_match_runtime_contract() {
        assert_eq!(
            exits_for_known_group("direct").unwrap(),
            vec![RouteExit::Direct]
        );
        assert_eq!(
            exits_for_known_group("free_pool").unwrap(),
            vec![
                RouteExit::FreePool,
                RouteExit::Warp,
                RouteExit::Xray,
                RouteExit::NoProxy
            ]
        );
        assert_eq!(
            exits_for_known_group("warp").unwrap(),
            vec![
                RouteExit::Warp,
                RouteExit::Xray,
                RouteExit::FreePool,
                RouteExit::NoProxy
            ]
        );
        assert_eq!(
            exits_for_known_group("xray").unwrap(),
            vec![
                RouteExit::Xray,
                RouteExit::FreePool,
                RouteExit::Warp,
                RouteExit::NoProxy
            ]
        );
        assert!(exits_for_known_group("custom").is_none());
    }

    #[test]
    fn geoip_candidate_orders_match_runtime_contract() {
        assert_eq!(geoip_exits(false), vec![RouteExit::Direct]);
        assert_eq!(
            geoip_exits(true),
            vec![
                RouteExit::Warp,
                RouteExit::Xray,
                RouteExit::FreePool,
                RouteExit::NoProxy
            ]
        );
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
}
