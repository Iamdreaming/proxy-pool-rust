// Proxy protocol types matching Rust backend
export type Protocol = 'http' | 'https' | 'socks4' | 'socks5'
export type Anonymity = 'transparent' | 'anonymous' | 'elite'

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
  status: 'ok' | 'error'
  message?: string
}

export interface WarpStatus {
  configured: number
  healthy: number
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

export interface RouteGroup {
  [group: string]: string[]
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
