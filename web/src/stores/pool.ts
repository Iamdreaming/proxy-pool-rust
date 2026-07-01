import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { Proxy, Protocol, PoolStatus } from '@/types'
import { fetchStatus, fetchProxies, fetchRandomProxy } from '@/api'

export const usePoolStore = defineStore('pool', () => {
  // -- State --
  const status = ref<PoolStatus>({ http: 0, https: 0, socks5: 0 })
  const proxies = ref<Proxy[]>([])
  const currentProtocol = ref<Protocol>('http')
  const loading = ref(false)
  const selectedProxy = ref<Proxy | null>(null)

  // -- Actions --
  async function loadStatus() {
    try {
      const resp = await fetchStatus()
      status.value = resp.pool
    } catch (e) {
      console.error('Failed to load status:', e)
    }
  }

  async function loadProxies(protocol?: Protocol, limit = 100) {
    loading.value = true
    try {
      const p = protocol || currentProtocol.value
      currentProtocol.value = p
      const resp = await fetchProxies(p, limit)
      proxies.value = resp.proxies
    } catch (e) {
      console.error('Failed to load proxies:', e)
    } finally {
      loading.value = false
    }
  }

  async function getRandomProxy(protocol?: Protocol) {
    try {
      const p = protocol || currentProtocol.value
      return await fetchRandomProxy(p)
    } catch (e) {
      console.error('Failed to get random proxy:', e)
      return null
    }
  }

  // -- Getters --
  const totalProxies = () => status.value.http + status.value.https + status.value.socks5
  const overseasProxies = () => proxies.value.filter(p => p.is_overseas).length
  const domesticProxies = () => proxies.value.filter(p => !p.is_overseas).length

  return {
    status,
    proxies,
    currentProtocol,
    loading,
    selectedProxy,
    loadStatus,
    loadProxies,
    getRandomProxy,
    totalProxies,
    overseasProxies,
    domesticProxies,
  }
})
