<template>
  <n-space vertical :size="16">
    <n-grid :cols="4" :x-gap="16" :y-gap="16" responsive="screen">
      <n-gi>
        <n-card>
          <n-statistic label="代理总数" :value="status?.pool.total ?? pool.totalProxies()" />
        </n-card>
      </n-gi>
      <n-gi>
        <n-card>
          <n-statistic label="Redis" :value="status?.redis.status ?? 'unknown'">
            <template #suffix>
              <n-tag :type="dependencyTag(status?.redis.status)" size="small">
                {{ status?.redis.status === 'ok' ? 'ready' : 'check' }}
              </n-tag>
            </template>
          </n-statistic>
        </n-card>
      </n-gi>
      <n-gi>
        <n-card>
          <n-statistic label="WARP 健康" :value="`${status?.warp.healthy ?? 0}/${status?.warp.configured ?? 0}`" />
        </n-card>
      </n-gi>
      <n-gi>
        <n-card>
          <n-statistic label="xray 活跃节点" :value="status?.xray.active_nodes ?? 0" />
        </n-card>
      </n-gi>
    </n-grid>

    <n-grid :cols="2" :x-gap="16" :y-gap="16" responsive="screen">
      <n-gi>
        <n-card title="服务状态">
          <template #header-extra>
            <n-button size="small" @click="loadOverview" :loading="loading">刷新</n-button>
          </template>
          <n-descriptions :column="1" bordered size="small">
            <n-descriptions-item label="版本">{{ status?.version ?? '-' }}</n-descriptions-item>
            <n-descriptions-item label="Git Hash">{{ shortHash(status?.git_hash) }}</n-descriptions-item>
            <n-descriptions-item label="运行时间">{{ formatUptime(status?.uptime_sec) }}</n-descriptions-item>
            <n-descriptions-item label="Readyz">
              <n-tag :type="dependencyTag(readiness?.status)" size="small">
                {{ readiness?.status ?? 'unknown' }}
              </n-tag>
              <span v-if="readiness?.message" class="muted">{{ readiness.message }}</span>
            </n-descriptions-item>
          </n-descriptions>
          <n-alert v-if="overviewError" type="error" :bordered="false" class="section-gap">
            {{ overviewError }}
          </n-alert>
        </n-card>
      </n-gi>

      <n-gi>
        <n-card title="协议分布">
          <n-space vertical>
            <div v-for="item in protocolData" :key="item.label" class="distribution-row">
              <span class="distribution-label">{{ item.label }}</span>
              <n-progress
                type="line"
                :percentage="item.pct"
                :color="item.color"
                :height="16"
                class="distribution-bar"
              />
              <span class="distribution-value">{{ item.value }}</span>
            </div>
          </n-space>
        </n-card>
      </n-gi>
    </n-grid>

    <n-grid :cols="2" :x-gap="16" :y-gap="16" responsive="screen">
      <n-gi>
        <n-card title="快捷操作">
          <n-space>
            <n-button type="primary" @click="handleRefresh" :loading="refreshing">刷新代理池</n-button>
            <n-button @click="$router.push('/fetchers')">抓取源状态</n-button>
            <n-button @click="$router.push('/routes')">路由 Dry-run</n-button>
            <n-button @click="$router.push('/proxies')">代理列表</n-button>
          </n-space>
        </n-card>
      </n-gi>
      <n-gi>
        <n-card title="依赖说明">
          <n-space vertical :size="8">
            <div>
              <n-tag :type="dependencyTag(status?.redis.status)" size="small">redis</n-tag>
              <span class="muted">{{ status?.redis.message || '代理池存储依赖' }}</span>
            </div>
            <div>
              <n-tag type="info" size="small">readyz</n-tag>
              <span class="muted">用于判断服务依赖是否可用，区别于进程存活。</span>
            </div>
          </n-space>
        </n-card>
      </n-gi>
    </n-grid>

    <n-card title="最近 HTTP 代理">
      <n-data-table
        :columns="recentColumns"
        :data="recentProxies"
        :bordered="false"
        size="small"
        :loading="pool.loading"
        :pagination="{ pageSize: 10 }"
      />
    </n-card>
  </n-space>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, h } from 'vue'
import { NTag, useMessage } from 'naive-ui'
import { fetchReadiness, fetchStatus, refreshPool } from '@/api'
import { usePoolStore } from '@/stores/pool'
import type { DependencyState, DependencyStatus, Proxy, StatusResponse } from '@/types'

const pool = usePoolStore()
const message = useMessage()
const loading = ref(false)
const refreshing = ref(false)
const status = ref<StatusResponse | null>(null)
const readiness = ref<DependencyStatus | null>(null)
const overviewError = ref('')

const recentProxies = computed(() => pool.proxies.slice(0, 20))

const protocolData = computed(() => {
  const counts = status.value?.pool ?? pool.status
  const total = counts.total || counts.http + counts.https + counts.socks5 || 1
  return [
    { label: 'HTTP', value: counts.http, pct: Math.round((counts.http / total) * 100), color: '#63e2b7' },
    { label: 'HTTPS', value: counts.https, pct: Math.round((counts.https / total) * 100), color: '#f2c97d' },
    { label: 'SOCKS5', value: counts.socks5, pct: Math.round((counts.socks5 / total) * 100), color: '#70c0e0' },
  ]
})

const recentColumns = [
  { title: '地址', key: 'host', width: 150 },
  { title: '端口', key: 'port', width: 80 },
  {
    title: '协议',
    key: 'protocol',
    width: 80,
    render: (row: Proxy) => h(NTag, { size: 'small', type: row.protocol === 'socks5' ? 'info' : 'success' }, { default: () => row.protocol }),
  },
  {
    title: '延迟',
    key: 'latency_ms',
    width: 100,
    render: (row: Proxy) => row.latency_ms ? `${row.latency_ms}ms` : '-',
  },
  {
    title: '匿名度',
    key: 'anonymity',
    width: 100,
    render: (row: Proxy) => {
      const typeMap: Record<string, 'success' | 'warning' | 'error'> = {
        elite: 'success',
        anonymous: 'warning',
        transparent: 'error',
      }
      return row.anonymity
        ? h(NTag, { size: 'small', type: typeMap[row.anonymity] || 'default' }, { default: () => row.anonymity })
        : '-'
    },
  },
  {
    title: '国家',
    key: 'country',
    width: 100,
    render: (row: Proxy) => row.country_name || row.country || '-',
  },
  {
    title: '成功/失败',
    key: 'success_rate',
    width: 100,
    render: (row: Proxy) => `${row.success_count}/${row.fail_count}`,
  },
]

function dependencyTag(state?: DependencyState): 'success' | 'error' | 'warning' {
  if (state === 'ok') return 'success'
  if (state === 'error') return 'error'
  return 'warning'
}

function shortHash(hash?: string): string {
  if (!hash) return '-'
  return hash.length > 12 ? hash.slice(0, 12) : hash
}

function formatUptime(seconds?: number): string {
  if (seconds === undefined) return '-'
  const days = Math.floor(seconds / 86400)
  const hours = Math.floor((seconds % 86400) / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  if (days > 0) return `${days}d ${hours}h ${minutes}m`
  if (hours > 0) return `${hours}h ${minutes}m`
  return `${minutes}m`
}

async function loadOverview() {
  loading.value = true
  overviewError.value = ''
  try {
    const [statusResp, readyResp] = await Promise.all([
      fetchStatus(),
      fetchReadiness(),
      pool.loadStatus(),
      pool.loadProxies('http', 20),
    ])
    status.value = statusResp
    readiness.value = readyResp
  } catch (e: any) {
    overviewError.value = e?.message || '加载服务状态失败'
    message.error('加载服务状态失败')
  } finally {
    loading.value = false
  }
}

async function handleRefresh() {
  refreshing.value = true
  try {
    await refreshPool()
    message.success('代理池刷新已触发')
    await loadOverview()
  } catch {
    message.error('刷新失败')
  } finally {
    refreshing.value = false
  }
}

onMounted(() => {
  loadOverview()
})
</script>

<style scoped>
.distribution-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.distribution-label {
  width: 72px;
  text-align: right;
  color: #8f8f9d;
}

.distribution-bar {
  flex: 1;
}

.distribution-value {
  width: 52px;
  color: #c7c7d4;
}

.muted {
  margin-left: 8px;
  color: #8f8f9d;
}

.section-gap {
  margin-top: 12px;
}
</style>
