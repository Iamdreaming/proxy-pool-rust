<template>
  <n-space vertical :size="16">
    <!-- Filter bar -->
    <n-card>
      <n-space>
        <n-select
          v-model:value="selectedProtocol"
          :options="protocolOptions"
          style="width: 140px"
          @update:value="loadProxies"
        />
        <n-select
          v-model:value="selectedPool"
          :options="poolOptions"
          style="width: 140px"
        />
        <n-input
          v-model:value="searchText"
          placeholder="搜索 IP / 端口"
          clearable
          style="width: 200px"
          @update:value="filterProxies"
        />
        <n-button type="primary" @click="loadProxies">查询</n-button>
        <n-button @click="handleRefresh">🔄 刷新池</n-button>
        <n-spin v-if="pool.loading" size="small" />
      </n-space>
    </n-card>

    <n-alert v-if="scoreError" type="warning" :bordered="false">
      {{ scoreError }}
    </n-alert>

    <!-- Proxies table -->
    <n-card :title="`代理列表 (${filteredProxies.length})`">
      <n-data-table
        :columns="columns"
        :data="filteredProxies"
        :bordered="false"
        size="small"
        :loading="pool.loading || loadingScores"
        :pagination="pagination"
        :row-key="(row: Proxy) => row.host + ':' + row.port"
      />
    </n-card>
  </n-space>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, h } from 'vue'
import { NTag, useMessage } from 'naive-ui'
import { usePoolStore } from '@/stores/pool'
import { fetchScoredProxies, refreshPool } from '@/api'
import type { Proxy, Protocol, RetentionDecision, ScoredProxy } from '@/types'

const pool = usePoolStore()
const message = useMessage()

const selectedProtocol = ref<Protocol>('http')
const selectedPool = ref('all')
const searchText = ref('')
const scoredProxies = ref<ScoredProxy[]>([])
const loadingScores = ref(false)
const scoreError = ref('')

const protocolOptions = [
  { label: 'HTTP', value: 'http' },
  { label: 'HTTPS', value: 'https' },
  { label: 'SOCKS5', value: 'socks5' },
]

const poolOptions = [
  { label: '全部', value: 'all' },
  { label: '境外', value: 'overseas' },
  { label: '境内', value: 'domestic' },
]

const pagination = { pageSize: 30 }

const scoredByKey = computed(() => {
  const map = new Map<string, ScoredProxy>()
  for (const item of scoredProxies.value) {
    map.set(proxyKey(item.proxy), item)
  }
  return map
})

const filteredProxies = computed(() => {
  let list = pool.proxies.length > 0 ? pool.proxies : scoredProxies.value.map(item => item.proxy)
  if (selectedPool.value === 'overseas') {
    list = list.filter(p => p.is_overseas)
  } else if (selectedPool.value === 'domestic') {
    list = list.filter(p => !p.is_overseas)
  }
  if (searchText.value) {
    const q = searchText.value.toLowerCase()
    list = list.filter(p => p.host.includes(q) || String(p.port).includes(q))
  }
  return list
})

function proxyKey(proxy: Proxy): string {
  return `${proxy.protocol}:${proxy.host}:${proxy.port}`
}

function retentionLabel(decision?: RetentionDecision): string {
  if (decision === 'keep') return '保留'
  if (decision === 'below_min_score') return '低分'
  if (decision === 'hard_failure_evict') return '失败淘汰'
  return '-'
}

function retentionType(decision?: RetentionDecision): 'success' | 'warning' | 'error' | 'default' {
  if (decision === 'keep') return 'success'
  if (decision === 'below_min_score') return 'warning'
  if (decision === 'hard_failure_evict') return 'error'
  return 'default'
}

const columns = [
  { title: '地址', key: 'host', width: 140, sorter: 'default' },
  { title: '端口', key: 'port', width: 70 },
  {
    title: '协议', key: 'protocol', width: 80,
    render: (row: Proxy) => h(NTag, { size: 'small', type: row.protocol === 'socks5' ? 'info' : 'success' }, { default: () => row.protocol }),
  },
  {
    title: '延迟', key: 'latency_ms', width: 90, sorter: (a: Proxy, b: Proxy) => (a.latency_ms || 9999) - (b.latency_ms || 9999),
    render: (row: Proxy) => {
      if (!row.latency_ms) return '-'
      const color = row.latency_ms < 500 ? '#63e2b7' : row.latency_ms < 1500 ? '#f2c97d' : '#e88080'
      return h('span', { style: `color: ${color}` }, `${row.latency_ms}ms`)
    },
  },
  {
    title: '匿名度', key: 'anonymity', width: 90,
    render: (row: Proxy) => {
      const typeMap: Record<string, any> = { elite: 'success', anonymous: 'warning', transparent: 'error' }
      return row.anonymity
        ? h(NTag, { size: 'small', type: typeMap[row.anonymity] || 'default' }, { default: () => row.anonymity })
        : '-'
    },
  },
  {
    title: '🇨🇳', key: 'is_overseas', width: 60,
    render: (row: Proxy) => row.is_overseas ? '🌍' : '🇨🇳',
  },
  {
    title: '国家', key: 'country_name', width: 100,
    render: (row: Proxy) => row.country_name || row.country || '-',
  },
  {
    title: '成功/失败', key: 'success_rate', width: 100,
    render: (row: Proxy) => `${row.success_count}/${row.fail_count}`,
  },
  {
    title: '评分',
    key: 'score',
    width: 90,
    sorter: (a: Proxy, b: Proxy) =>
      (scoredByKey.value.get(proxyKey(a))?.score.score ?? -1) -
      (scoredByKey.value.get(proxyKey(b))?.score.score ?? -1),
    render: (row: Proxy) => {
      const scored = scoredByKey.value.get(proxyKey(row))
      return scored ? scored.score.score.toFixed(3) : '-'
    },
  },
  {
    title: '保留决策',
    key: 'retention',
    width: 100,
    render: (row: Proxy) => {
      const decision = scoredByKey.value.get(proxyKey(row))?.score.retention
      return h(NTag, { size: 'small', type: retentionType(decision) }, { default: () => retentionLabel(decision) })
    },
  },
  {
    title: '评分原因',
    key: 'score_reason',
    width: 180,
    render: (row: Proxy) => {
      const score = scoredByKey.value.get(proxyKey(row))?.score
      if (!score) return '-'
      const latency = score.latency.contribution.toFixed(2)
      const success = score.success.contribution.toFixed(2)
      const anonymity = score.anonymity.contribution.toFixed(2)
      return `延迟 ${latency} / 成功 ${success} / 匿名 ${anonymity}`
    },
  },
  {
    title: '来源', key: 'source', width: 100,
    render: (row: Proxy) => row.source || '-',
  },
]

async function loadProxies() {
  await pool.loadProxies(selectedProtocol.value, 500)
  await loadScores()
}

async function loadScores() {
  loadingScores.value = true
  scoreError.value = ''
  try {
    const resp = await fetchScoredProxies(selectedProtocol.value, 500)
    scoredProxies.value = resp.proxies
  } catch (e: any) {
    scoreError.value = e?.message || '评分解释加载失败'
    scoredProxies.value = []
  } finally {
    loadingScores.value = false
  }
}

function filterProxies() {
  // computed handles it
}

async function handleRefresh() {
  try {
    await refreshPool()
    message.success('刷新已触发')
    await loadProxies()
  } catch {
    message.error('刷新失败')
  }
}

onMounted(() => {
  loadProxies()
})
</script>
