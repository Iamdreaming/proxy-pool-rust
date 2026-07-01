<template>
  <n-space vertical :size="24">
    <!-- Top stats cards -->
    <n-grid :cols="4" :x-gap="16" :y-gap="16">
      <n-gi>
        <n-card>
          <n-statistic label="HTTP 代理" :value="pool.status.http">
            <template #prefix>🌐</template>
          </n-statistic>
        </n-card>
      </n-gi>
      <n-gi>
        <n-card>
          <n-statistic label="HTTPS 代理" :value="pool.status.https">
            <template #prefix>🔒</template>
          </n-statistic>
        </n-card>
      </n-gi>
      <n-gi>
        <n-card>
          <n-statistic label="SOCKS5 代理" :value="pool.status.socks5">
            <template #prefix>🧦</template>
          </n-statistic>
        </n-card>
      </n-gi>
      <n-gi>
        <n-card>
          <n-statistic label="代理总数" :value="total">
            <template #prefix>📊</template>
          </n-statistic>
        </n-card>
      </n-gi>
    </n-grid>

    <!-- Protocol distribution + Quick actions -->
    <n-grid :cols="2" :x-gap="16">
      <n-gi>
        <n-card title="协议分布">
          <n-space vertical>
            <div v-for="item in protocolData" :key="item.label" style="display: flex; align-items: center; gap: 12px">
              <span style="width: 80px; text-align: right; color: #aaa">{{ item.label }}</span>
              <n-progress
                type="line"
                :percentage="item.pct"
                :color="item.color"
                :height="18"
                style="flex: 1"
              />
              <span style="width: 60px; color: #ccc">{{ item.value }}</span>
            </div>
          </n-space>
        </n-card>
      </n-gi>
      <n-gi>
        <n-card title="快捷操作">
          <n-space vertical :size="12">
            <n-button type="primary" block @click="handleRefresh" :loading="refreshing">
              🔄 刷新代理池
            </n-button>
            <n-button block @click="$router.push('/proxies')">
              🌐 查看代理列表
            </n-button>
            <n-button block @click="$router.push('/mcp')">
              🤖 MCP 调试面板
            </n-button>
            <n-button block @click="$router.push('/warp')">
              ☁️ WARP 管理
            </n-button>
          </n-space>
        </n-card>
      </n-gi>
    </n-grid>

    <!-- Recent proxies table -->
    <n-card title="最近验证的代理">
      <n-data-table
        :columns="recentColumns"
        :data="recentProxies"
        :bordered="false"
        size="small"
        :pagination="{ pageSize: 10 }"
      />
    </n-card>
  </n-space>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, h } from 'vue'
import { NTag, NButton, useMessage } from 'naive-ui'
import { usePoolStore } from '@/stores/pool'
import { refreshPool } from '@/api'
import type { Proxy } from '@/types'

const pool = usePoolStore()
const message = useMessage()
const refreshing = ref(false)
const recentProxies = ref<Proxy[]>([])

const total = computed(() => pool.totalProxies())

const protocolData = computed(() => {
  const t = total.value || 1
  return [
    { label: 'HTTP', value: pool.status.http, pct: Math.round((pool.status.http / t) * 100), color: '#63e2b7' },
    { label: 'HTTPS', value: pool.status.https, pct: Math.round((pool.status.https / t) * 100), color: '#f2c97d' },
    { label: 'SOCKS5', value: pool.status.socks5, pct: Math.round((pool.status.socks5 / t) * 100), color: '#70c0e0' },
  ]
})

const recentColumns = [
  { title: '地址', key: 'host', width: 150 },
  { title: '端口', key: 'port', width: 80 },
  {
    title: '协议', key: 'protocol', width: 80,
    render: (row: Proxy) => h(NTag, { size: 'small', type: row.protocol === 'socks5' ? 'info' : 'success' }, { default: () => row.protocol }),
  },
  {
    title: '延迟', key: 'latency_ms', width: 100,
    render: (row: Proxy) => row.latency_ms ? `${row.latency_ms}ms` : '-',
  },
  {
    title: '匿名度', key: 'anonymity', width: 100,
    render: (row: Proxy) => {
      const typeMap: Record<string, any> = { elite: 'success', anonymous: 'warning', transparent: 'error' }
      return row.anonymity
        ? h(NTag, { size: 'small', type: typeMap[row.anonymity] || 'default' }, { default: () => row.anonymity })
        : '-'
    },
  },
  {
    title: '国家', key: 'country', width: 80,
    render: (row: Proxy) => row.country_name || row.country || '-',
  },
  {
    title: '成功率', key: 'success_rate', width: 100,
    render: (row: Proxy) => {
      const total = row.success_count + row.fail_count
      return total > 0 ? `${Math.round((row.success_count / total) * 100)}%` : '-'
    },
  },
]

async function handleRefresh() {
  refreshing.value = true
  try {
    await refreshPool()
    message.success('代理池刷新已触发')
    setTimeout(() => pool.loadStatus(), 2000)
  } catch {
    message.error('刷新失败')
  } finally {
    refreshing.value = false
  }
}

onMounted(async () => {
  await pool.loadStatus()
  await pool.loadProxies('http', 20)
  recentProxies.value = pool.proxies.slice(0, 20)
})
</script>
