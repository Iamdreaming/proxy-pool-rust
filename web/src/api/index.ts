import axios from 'axios'
import type { StatusResponse, ProxiesResponse, Proxy, Protocol } from '@/types'

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

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

export async function fetchRoutes(): Promise<Record<string, string[]>> {
  const { data } = await api.get('/routes')
  return data.groups || data
}

export async function updateRoutes(groups: Record<string, string[]>): Promise<void> {
  await api.put('/routes', { groups })
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
