<template>
  <n-space vertical :size="16">
    <n-grid :cols="3" :x-gap="16">
      <n-gi>
        <n-card>
          <n-statistic label="健康实例" :value="healthyCount">
            <template #prefix>✅</template>
          </n-statistic>
        </n-card>
      </n-gi>
      <n-gi>
        <n-card>
          <n-statistic label="总实例数" :value="instances.length">
            <template #prefix>☁️</template>
          </n-statistic>
        </n-card>
      </n-gi>
      <n-gi>
        <n-card>
          <n-statistic label="健康率" :value="healthRate">
            <template #suffix>%</template>
          </n-statistic>
        </n-card>
      </n-gi>
    </n-grid>

    <n-card title="WARP 实例">
      <n-data-table
        :columns="columns"
        :data="instances"
        :bordered="false"
        size="small"
      />
    </n-card>

    <n-card title="操作">
      <n-space>
        <n-button type="primary" @click="fetchWarpStatus">🔄 刷新状态</n-button>
        <n-button disabled>优选暂无 Web API</n-button>
      </n-space>
    </n-card>
  </n-space>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, h } from 'vue'
import { NTag, NButton, useMessage } from 'naive-ui'
import { fetchWarpInstances } from '@/api'
import type { WarpInstance } from '@/types'

const message = useMessage()
const instances = ref<WarpInstance[]>([])

const healthyCount = computed(() => instances.value.filter(i => i.healthy).length)
const healthRate = computed(() => {
  if (instances.value.length === 0) return 0
  return Math.round((healthyCount.value / instances.value.length) * 100)
})

const columns = [
  { title: 'ID', key: 'id', width: 60 },
  { title: 'SOCKS5 端口', key: 'socks5_port', width: 120 },
  {
    title: '状态', key: 'healthy', width: 100,
    render: (row: WarpInstance) =>
      h(NTag, { type: row.healthy ? 'success' : 'error', size: 'small' }, { default: () => row.healthy ? '健康' : '故障' }),
  },
  { title: '连续失败', key: 'fail_streak', width: 100 },
  {
    title: '当前端点', key: 'endpoint', width: 200,
    render: (row: WarpInstance) => row.endpoint ? `${row.endpoint.ip}:${row.endpoint.port}` : '-',
  },
  {
    title: '端点延迟', key: 'endpoint_latency', width: 100,
    render: (row: WarpInstance) => row.endpoint ? `${row.endpoint.latency_ms.toFixed(0)}ms` : '-',
  },
  {
    title: '端点丢包', key: 'endpoint_loss', width: 100,
    render: (row: WarpInstance) => row.endpoint ? `${row.endpoint.loss_pct.toFixed(1)}%` : '-',
  },
]

async function fetchWarpStatus() {
  try {
    const data = await fetchWarpInstances()
    instances.value = data.instances
  } catch (e) {
    message.error('获取 WARP 状态失败')
  }
}

onMounted(() => {
  fetchWarpStatus()
})
</script>
