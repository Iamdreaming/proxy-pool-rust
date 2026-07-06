<template>
  <n-space vertical :size="16">
    <n-grid :cols="4" :x-gap="16" :y-gap="16" responsive="screen">
      <n-gi>
        <n-card>
          <n-statistic label="抓取源" :value="fetchers.length" />
        </n-card>
      </n-gi>
      <n-gi>
        <n-card>
          <n-statistic label="成功" :value="countByStatus('success')" />
        </n-card>
      </n-gi>
      <n-gi>
        <n-card>
          <n-statistic label="空结果" :value="countByStatus('empty')" />
        </n-card>
      </n-gi>
      <n-gi>
        <n-card>
          <n-statistic label="错误" :value="countByStatus('error')" />
        </n-card>
      </n-gi>
    </n-grid>

    <n-card title="抓取源状态">
      <template #header-extra>
        <n-button size="small" @click="loadFetchers" :loading="loading">刷新状态</n-button>
      </template>

      <n-alert v-if="lastRefresh" type="success" :bordered="false" class="section-gap">
        {{ lastRefresh }}
      </n-alert>
      <n-alert v-if="error" type="error" :bordered="false" class="section-gap">
        {{ error }}
      </n-alert>

      <n-data-table
        :columns="columns"
        :data="fetchers"
        :bordered="false"
        size="small"
        :loading="loading"
        :pagination="{ pageSize: 20 }"
        :row-key="(row: FetcherRunReport) => row.id"
      />
    </n-card>
  </n-space>
</template>

<script setup lang="ts">
import { computed, h, onMounted, ref } from 'vue'
import { NButton, NTag, useMessage } from 'naive-ui'
import { fetchFetcherStatus, refreshFetcher } from '@/api'
import type { FetcherRunReport, FetcherRunStatus } from '@/types'

const message = useMessage()
const fetchers = ref<FetcherRunReport[]>([])
const loading = ref(false)
const refreshingId = ref('')
const error = ref('')
const lastRefresh = ref('')

const columns = computed(() => [
  { title: 'ID', key: 'id', width: 170 },
  { title: '名称', key: 'name', width: 180 },
  {
    title: '状态',
    key: 'status',
    width: 100,
    render: (row: FetcherRunReport) =>
      h(NTag, { size: 'small', type: statusTag(row.status) }, { default: () => statusLabel(row.status) }),
  },
  { title: '抓取', key: 'fetched', width: 80 },
  { title: '解析', key: 'parsed', width: 80 },
  {
    title: '耗时',
    key: 'duration_ms',
    width: 100,
    render: (row: FetcherRunReport) => row.duration_ms === undefined ? '-' : `${row.duration_ms}ms`,
  },
  {
    title: '最近完成',
    key: 'finished_at',
    width: 180,
    render: (row: FetcherRunReport) => formatTime(row.finished_at),
  },
  {
    title: '错误',
    key: 'error',
    ellipsis: { tooltip: true },
    render: (row: FetcherRunReport) => row.error || '-',
  },
  {
    title: '操作',
    key: 'actions',
    width: 120,
    render: (row: FetcherRunReport) =>
      h(NButton, {
        size: 'small',
        quaternary: true,
        type: 'primary',
        loading: refreshingId.value === row.id,
        onClick: () => refreshOne(row.id),
      }, { default: () => '刷新' }),
  },
])

function statusTag(status: FetcherRunStatus): 'success' | 'warning' | 'error' | 'default' {
  if (status === 'success') return 'success'
  if (status === 'empty' || status === 'never_run') return 'warning'
  if (status === 'error') return 'error'
  return 'default'
}

function statusLabel(status: FetcherRunStatus): string {
  if (status === 'never_run') return '未运行'
  if (status === 'success') return '成功'
  if (status === 'empty') return '空'
  if (status === 'error') return '错误'
  return status
}

function countByStatus(status: FetcherRunStatus): number {
  return fetchers.value.filter(item => item.status === status).length
}

function formatTime(value?: string): string {
  if (!value) return '-'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return date.toLocaleString()
}

async function loadFetchers() {
  loading.value = true
  error.value = ''
  try {
    const resp = await fetchFetcherStatus()
    fetchers.value = resp.fetchers
  } catch (e: any) {
    error.value = e?.message || '加载抓取源状态失败'
    message.error('加载抓取源状态失败')
  } finally {
    loading.value = false
  }
}

async function refreshOne(id: string) {
  refreshingId.value = id
  error.value = ''
  lastRefresh.value = ''
  try {
    const resp = await refreshFetcher(id)
    lastRefresh.value = `${id} 刷新完成：fetched=${resp.fetched}, validated=${resp.validated}, stored=${resp.stored}, errors=${resp.errors}`
    fetchers.value = resp.fetchers.length > 0 ? resp.fetchers : fetchers.value
    await loadFetchers()
  } catch (e: any) {
    error.value = e?.response?.data?.status || e?.message || `${id} 刷新失败`
    message.error(`${id} 刷新失败`)
  } finally {
    refreshingId.value = ''
  }
}

onMounted(() => {
  loadFetchers()
})
</script>

<style scoped>
.section-gap {
  margin-bottom: 12px;
}
</style>
