<template>
  <n-space vertical :size="16">
    <n-card title="路由 Dry-run">
      <n-alert type="info" :bordered="false" class="section-gap">
        当前 Web 端只提供真实路由诊断。路由规则仍通过 config/settings.yaml 配置，后端尚未提供规则读写 API。
      </n-alert>

      <n-form label-placement="left" label-width="90" class="section-gap">
        <n-grid :cols="3" :x-gap="12" :y-gap="12" responsive="screen">
          <n-gi :span="2">
            <n-form-item label="目标主机">
              <n-input v-model:value="host" placeholder="example.com" @keyup.enter="runDryRun" />
            </n-form-item>
          </n-gi>
          <n-gi>
            <n-form-item label="协议">
              <n-select v-model:value="protocol" :options="protocolOptions" />
            </n-form-item>
          </n-gi>
        </n-grid>
      </n-form>

      <n-space>
        <n-button type="primary" @click="runDryRun" :loading="loading">执行 Dry-run</n-button>
        <n-button @click="resetForm">重置</n-button>
      </n-space>

      <n-alert v-if="error" type="error" :bordered="false" class="section-gap">
        {{ error }}
      </n-alert>
    </n-card>

    <n-card v-if="decision" title="路由决策">
      <n-descriptions :column="2" bordered size="small">
        <n-descriptions-item label="Host">{{ decision.host }}</n-descriptions-item>
        <n-descriptions-item label="Protocol">{{ decision.protocol }}</n-descriptions-item>
        <n-descriptions-item label="Matched Group">{{ decision.matched_group || '-' }}</n-descriptions-item>
        <n-descriptions-item label="Matched Rule">{{ decision.matched_rule || '-' }}</n-descriptions-item>
        <n-descriptions-item label="Reason">{{ decision.matched_reason }}</n-descriptions-item>
        <n-descriptions-item label="Selected">
          <n-tag :type="exitTag(decision.selected)" size="small">{{ decision.selected }}</n-tag>
        </n-descriptions-item>
        <n-descriptions-item label="GeoIP">
          <span v-if="decision.geoip">
            {{ decision.geoip.country_name }} ({{ decision.geoip.country }})
            {{ decision.geoip.overseas ? 'overseas' : 'domestic' }}
          </span>
          <span v-else>-</span>
        </n-descriptions-item>
      </n-descriptions>
    </n-card>

    <n-grid v-if="decision" :cols="2" :x-gap="16" :y-gap="16" responsive="screen">
      <n-gi>
        <n-card title="候选出口">
          <n-data-table
            :columns="candidateColumns"
            :data="decision.candidates"
            :bordered="false"
            size="small"
            :pagination="false"
          />
        </n-card>
      </n-gi>
      <n-gi>
        <n-card title="不可用出口">
          <n-empty v-if="decision.unavailable.length === 0" description="没有不可用候选" />
          <n-data-table
            v-else
            :columns="unavailableColumns"
            :data="decision.unavailable"
            :bordered="false"
            size="small"
            :pagination="false"
          />
        </n-card>
      </n-gi>
    </n-grid>
  </n-space>
</template>

<script setup lang="ts">
import { ref, h } from 'vue'
import { NTag, useMessage } from 'naive-ui'
import { testRoute } from '@/api'
import type { Protocol, RouteCandidate, RouteDecision, RouteExit, RouteUnavailable } from '@/types'

const message = useMessage()
const host = ref('github.com')
const protocol = ref<Protocol>('http')
const loading = ref(false)
const error = ref('')
const decision = ref<RouteDecision | null>(null)

const protocolOptions = [
  { label: 'HTTP', value: 'http' },
  { label: 'HTTPS', value: 'https' },
  { label: 'SOCKS5', value: 'socks5' },
]

const candidateColumns = [
  { title: '优先级', key: 'priority', width: 80 },
  {
    title: '出口',
    key: 'exit',
    width: 110,
    render: (row: RouteCandidate) =>
      h(NTag, { size: 'small', type: exitTag(row.exit) }, { default: () => row.exit }),
  },
  {
    title: '可用',
    key: 'available',
    width: 80,
    render: (row: RouteCandidate) =>
      h(NTag, { size: 'small', type: row.available ? 'success' : 'warning' }, { default: () => row.available ? 'yes' : 'no' }),
  },
  { title: '来源', key: 'source', width: 150 },
  {
    title: '说明',
    key: 'reason',
    render: (row: RouteCandidate) => row.reason || row.detail || '-',
  },
]

const unavailableColumns = [
  {
    title: '出口',
    key: 'exit',
    width: 120,
    render: (row: RouteUnavailable) =>
      h(NTag, { size: 'small', type: exitTag(row.exit) }, { default: () => row.exit }),
  },
  { title: '原因', key: 'reason' },
]

function exitTag(exit: RouteExit): 'success' | 'info' | 'warning' | 'error' | 'default' {
  if (exit === 'direct') return 'success'
  if (exit === 'free_pool') return 'info'
  if (exit === 'warp' || exit === 'xray') return 'warning'
  if (exit === 'no_proxy') return 'error'
  return 'default'
}

async function runDryRun() {
  const target = host.value.trim()
  if (!target) {
    message.warning('目标主机不能为空')
    return
  }

  loading.value = true
  error.value = ''
  try {
    const resp = await testRoute(target, protocol.value)
    decision.value = resp.decision
    if (!resp.decision) {
      error.value = resp.status || '路由诊断没有返回决策'
    }
  } catch (e: any) {
    error.value = e?.response?.data?.status || e?.message || '路由诊断失败'
    decision.value = null
  } finally {
    loading.value = false
  }
}

function resetForm() {
  host.value = 'github.com'
  protocol.value = 'http'
  error.value = ''
  decision.value = null
}
</script>

<style scoped>
.section-gap {
  margin-bottom: 16px;
}
</style>
