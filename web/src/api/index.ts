import axios from 'axios'
import type {
  DependencyStatus,
  FetchersResponse,
  ProxiesResponse,
  Proxy,
  Protocol,
  RefreshResponse,
  RouteTestResponse,
  ScoredProxiesResponse,
  StatusResponse,
  WarpInstancesResponse,
} from '@/types'

const api = axios.create({
  baseURL: '/api',
  timeout: 30000,
})

// ---------------------------------------------------------------------------
// Pool status
// ---------------------------------------------------------------------------

export async function fetchStatus(): Promise<StatusResponse> {
  const { data } = await api.get<StatusResponse>('/status')
  return data
}

export async function fetchReadiness(): Promise<DependencyStatus> {
  const { data } = await api.get<DependencyStatus>('/readyz')
  return data
}

// ---------------------------------------------------------------------------
// Proxies
// ---------------------------------------------------------------------------

export async function fetchProxies(
  protocol: Protocol = 'http',
  limit = 50,
): Promise<ProxiesResponse> {
  const { data } = await api.get<ProxiesResponse>('/proxies', {
    params: { protocol, limit },
  })
  return data
}

export async function fetchScoredProxies(
  protocol: Protocol = 'http',
  limit = 50,
): Promise<ScoredProxiesResponse> {
  const { data } = await api.get<ScoredProxiesResponse>('/proxies/scores', {
    params: { protocol, limit },
  })
  return data
}

export async function fetchRandomProxy(
  protocol: Protocol = 'http',
): Promise<Proxy | null> {
  const { data } = await api.get<Proxy | null>('/proxy/random', {
    params: { protocol },
  })
  return data
}

export async function fetchBestProxy(
  protocol: Protocol = 'http',
): Promise<Proxy | null> {
  const { data } = await api.get<Proxy | null>('/proxy/best', {
    params: { protocol },
  })
  return data
}

export async function refreshPool(): Promise<void> {
  await api.post('/proxies/refresh')
}

export async function deleteProxy(protocol: Protocol, host: string, port: number): Promise<void> {
  const key = `${protocol}:${host}:${port}`
  await api.delete(`/proxy/${encodeURIComponent(key)}`)
}

// ---------------------------------------------------------------------------
// Fetchers
// ---------------------------------------------------------------------------

export async function fetchFetcherStatus(): Promise<FetchersResponse> {
  const { data } = await api.get<FetchersResponse>('/fetchers')
  return data
}

export async function refreshFetcher(id: string): Promise<RefreshResponse> {
  const { data } = await api.post<RefreshResponse>(`/fetchers/${encodeURIComponent(id)}/refresh`)
  return data
}

export async function testRoute(host: string, protocol: Protocol = 'http'): Promise<RouteTestResponse> {
  const { data } = await api.get<RouteTestResponse>('/routes/test', {
    params: { host, protocol },
  })
  return data
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

export async function fetchMetrics(): Promise<string> {
  const { data } = await api.get<string>('/metrics', {
    responseType: 'text',
  })
  return data
}

// ---------------------------------------------------------------------------
// WARP
// ---------------------------------------------------------------------------

export async function fetchWarpInstances(): Promise<WarpInstancesResponse> {
  const { data } = await api.get<WarpInstancesResponse>('/warp')
  return data
}
