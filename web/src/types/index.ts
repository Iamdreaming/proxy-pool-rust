// Proxy protocol types matching Rust backend
export type Protocol = 'http' | 'https' | 'socks4' | 'socks5'
export type Anonymity = 'transparent' | 'anonymous' | 'elite'
export type DependencyState = 'ok' | 'error'
export type FetcherRunStatus = 'never_run' | 'success' | 'empty' | 'error'
export type RetentionDecision = 'keep' | 'below_min_score' | 'hard_failure_evict'
export type RouteExit = 'direct' | 'free_pool' | 'warp' | 'xray' | 'no_proxy'

export interface Proxy {
  host: string
  port: number
  protocol: Protocol
  latency_ms: number | null
  anonymity: Anonymity | null
  last_check: string | null
  success_count: number
  fail_count: number
  country: string | null
  country_name: string | null
  is_overseas: boolean
  warp_chain_ok: boolean
  warp_chain_latency_ms: number | null
  circuit_open: boolean
  source: string | null
}

export interface WarpEndpoint {
  ip: string
  port: number
  loss_pct: number
  latency_ms: number
}

export interface WarpInstance {
  id: number
  socks5_port: number
  endpoint: WarpEndpoint | null
  healthy: boolean
  fail_streak: number
  last_optimized: string | null
}

export interface PoolStatus {
  http: number
  https: number
  socks5: number
  total: number
}

export interface DependencyStatus {
  status: DependencyState
  message?: string
}

export interface WarpStatus {
  configured: number
  healthy: number
}

export interface WarpInstancesResponse {
  instances: WarpInstance[]
}

export interface XrayStatus {
  active_nodes: number
}

export interface StatusResponse {
  version: string
  git_hash: string
  uptime_sec: number
  pool: PoolStatus
  redis: DependencyStatus
  warp: WarpStatus
  xray: XrayStatus
}

export interface ProxiesResponse {
  protocol: string
  count: number
  proxies: Proxy[]
}

export interface ScoreComponent {
  normalized: number
  weight: number
  contribution: number
}

export interface LatencyScoreComponent extends ScoreComponent {
  latency_ms: number | null
}

export interface SuccessScoreComponent extends ScoreComponent {
  success_count: number
  fail_count: number
  success_rate: number
}

export interface AnonymityScoreComponent extends ScoreComponent {
  anonymity: Anonymity | null
}

export interface ScoreExplanation {
  score: number
  min_score: number
  latency: LatencyScoreComponent
  success: SuccessScoreComponent
  anonymity: AnonymityScoreComponent
  retention: RetentionDecision
}

export interface ScoredProxy {
  proxy: Proxy
  score: ScoreExplanation
}

export interface ScoredProxiesResponse {
  protocol: string
  count: number
  proxies: ScoredProxy[]
}

export interface RouteGroup {
  [group: string]: string[]
}

export interface RouteCandidate {
  exit: RouteExit
  priority: number
  source: string
  available: boolean
  reason?: string
  detail?: string
}

export interface RouteUnavailable {
  exit: RouteExit
  reason: string
}

export interface RouteGeoIpDecision {
  country: string
  country_name: string
  overseas: boolean
}

export interface RouteDecision {
  host: string
  protocol: string
  matched_group?: string
  matched_rule?: string
  matched_reason: string
  geoip?: RouteGeoIpDecision
  candidates: RouteCandidate[]
  selected: RouteExit
  unavailable: RouteUnavailable[]
}

export interface RouteTestResponse {
  status: string
  decision: RouteDecision | null
}

export interface FetcherRunReport {
  id: string
  name: string
  status: FetcherRunStatus
  fetched: number
  parsed: number
  error?: string
  started_at?: string
  finished_at?: string
  duration_ms?: number
}

export interface FetchersResponse {
  fetchers: FetcherRunReport[]
}

export interface RefreshResponse {
  status: string
  fetched: number
  validated: number
  stored: number
  errors: number
  fetchers: FetcherRunReport[]
}

export interface McpTool {
  name: string
  description: string
  parameters: McpToolParam[]
}

export interface McpToolParam {
  name: string
  type: string
  description: string
  required: boolean
}

export interface McpCallResult {
  content: string
  isError: boolean
}
