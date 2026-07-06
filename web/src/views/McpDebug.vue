<template>
  <n-space vertical :size="16">
    <n-grid :cols="3" :x-gap="16" responsive="screen">
      <n-gi :span="1">
        <n-card title="MCP Tools">
          <n-input v-model:value="toolSearch" placeholder="搜索工具..." clearable class="search-box" />
          <n-space vertical :size="8">
            <n-card
              v-for="tool in filteredTools"
              :key="tool.name"
              size="small"
              :class="{ 'tool-card': true, 'tool-active': selectedTool?.name === tool.name }"
              hoverable
              @click="selectTool(tool)"
            >
              <div class="tool-name">{{ tool.name }}</div>
              <div class="tool-description">{{ tool.description }}</div>
            </n-card>
          </n-space>
        </n-card>
      </n-gi>

      <n-gi :span="2">
        <n-card v-if="selectedTool" :title="`调用: ${selectedTool.name}`">
          <template #header-extra>
            <n-button type="primary" @click="executeTool" :loading="executing">执行</n-button>
          </template>

          <n-space vertical :size="16">
            <n-alert :bordered="false" type="info">
              {{ selectedTool.description }}
            </n-alert>

            <n-form v-if="selectedTool.parameters.length > 0" label-placement="left" label-width="120">
              <n-form-item
                v-for="param in selectedTool.parameters"
                :key="param.name"
                :label="param.name"
              >
                <n-input
                  v-model:value="paramValues[param.name]"
                  :placeholder="param.description || `${param.type}`"
                />
                <template #label>
                  <span>{{ param.name }}</span>
                  <n-tag v-if="param.required" size="tiny" type="error" class="required-tag">必填</n-tag>
                </template>
              </n-form-item>
            </n-form>

            <n-divider v-if="lastResult" />

            <div v-if="lastResult">
              <div class="result-header">
                <span class="result-title">返回结果</span>
                <n-tag :type="lastResult.isError ? 'error' : 'success'" size="small">
                  {{ lastResult.isError ? '失败' : '成功' }}
                </n-tag>
              </div>
              <n-code
                :code="formatResult(lastResult.content)"
                language="json"
                :word-wrap="true"
                class="result-code"
              />
            </div>
          </n-space>
        </n-card>

        <n-card v-else title="MCP 调试面板">
          <n-empty description="选择左侧工具开始调试" />
        </n-card>
      </n-gi>
    </n-grid>

    <n-card title="调用历史">
      <n-data-table
        :columns="historyColumns"
        :data="callHistory"
        :bordered="false"
        size="small"
        :pagination="{ pageSize: 10 }"
      />
    </n-card>
  </n-space>
</template>

<script setup lang="ts">
import { computed, h, ref } from 'vue'
import { NTag, useMessage } from 'naive-ui'
import {
  deleteProxy,
  fetchBestProxy,
  fetchFetcherStatus,
  fetchProxies,
  fetchRandomProxy,
  fetchScoredProxies,
  fetchStatus,
  fetchWarpInstances,
  refreshFetcher,
  refreshPool,
  testRoute,
} from '@/api'
import type { McpCallResult, McpTool, Protocol } from '@/types'

const message = useMessage()
const toolSearch = ref('')
const selectedTool = ref<McpTool | null>(null)
const paramValues = ref<Record<string, string>>({})
const executing = ref(false)
const lastResult = ref<McpCallResult | null>(null)
const callHistory = ref<Array<{ time: string; tool: string; params: string; result: string; isError: boolean }>>([])

const mcpTools: McpTool[] = [
  {
    name: 'get_proxy',
    description: '获取一个可用代理（REST 等效调用）',
    parameters: [
      { name: 'protocol', type: 'string', description: 'http / https / socks4 / socks5', required: false },
    ],
  },
  {
    name: 'get_best_proxy',
    description: '获取评分最高的代理（REST 等效调用）',
    parameters: [
      { name: 'protocol', type: 'string', description: 'http / https / socks4 / socks5', required: false },
    ],
  },
  {
    name: 'list_proxies',
    description: '列出代理池代理（REST 等效调用）',
    parameters: [
      { name: 'protocol', type: 'string', description: '协议过滤', required: false },
      { name: 'limit', type: 'number', description: '返回数量，默认 20', required: false },
    ],
  },
  {
    name: 'check_proxy',
    description: '验证指定代理可用性（仅 MCP transport）',
    parameters: [
      { name: 'host', type: 'string', description: '代理主机', required: true },
      { name: 'port', type: 'number', description: '代理端口', required: true },
      { name: 'protocol', type: 'string', description: '代理协议', required: true },
    ],
  },
  { name: 'service_status', description: '查看完整服务状态（REST 等效调用）', parameters: [] },
  { name: 'pool_status', description: '查看代理池状态概览（REST 等效调用）', parameters: [] },
  { name: 'warp_status', description: '查看 WARP 实例状态（REST 等效调用）', parameters: [] },
  {
    name: 'geoip_lookup',
    description: '查询主机地理位置（仅 MCP transport）',
    parameters: [
      { name: 'host', type: 'string', description: '主机名或 IP', required: true },
    ],
  },
  {
    name: 'remove_proxy',
    description: '从池中移除代理（REST 等效调用）',
    parameters: [
      { name: 'host', type: 'string', description: '代理主机', required: true },
      { name: 'port', type: 'number', description: '代理端口', required: true },
      { name: 'protocol', type: 'string', description: '代理协议', required: true },
    ],
  },
  { name: 'refresh_pool', description: '触发全量抓取和验证（REST 等效调用）', parameters: [] },
  { name: 'proxy_stats', description: '查看代理池统计（REST 等效调用）', parameters: [] },
  { name: 'fetcher_status', description: '查看抓取源状态（REST 等效调用）', parameters: [] },
  {
    name: 'refresh_fetcher',
    description: '刷新单个抓取源（REST 等效调用）',
    parameters: [
      { name: 'fetcher', type: 'string', description: 'fetcher id', required: true },
    ],
  },
  {
    name: 'route_test',
    description: '执行路由 dry-run（REST 等效调用）',
    parameters: [
      { name: 'host', type: 'string', description: '目标主机', required: true },
      { name: 'protocol', type: 'string', description: 'http / https / socks5', required: false },
    ],
  },
  {
    name: 'explain_proxy_scores',
    description: '查看代理评分解释（REST 等效调用）',
    parameters: [
      { name: 'protocol', type: 'string', description: '协议过滤', required: false },
      { name: 'limit', type: 'number', description: '返回数量，默认 20', required: false },
    ],
  },
  {
    name: 'cleanup_low_score_proxies',
    description: '低分代理清理 dry-run/apply（仅 MCP transport）',
    parameters: [
      { name: 'protocol', type: 'string', description: '协议过滤', required: false },
      { name: 'limit', type: 'number', description: '扫描数量', required: false },
      { name: 'min_score', type: 'number', description: '最低分阈值', required: false },
      { name: 'apply', type: 'boolean', description: 'true 才实际删除', required: false },
    ],
  },
  { name: 'update_service', description: '触发容器自更新（仅 MCP transport）', parameters: [] },
]

const filteredTools = computed(() => {
  if (!toolSearch.value) return mcpTools
  const q = toolSearch.value.toLowerCase()
  return mcpTools.filter(t =>
    t.name.toLowerCase().includes(q) || t.description.toLowerCase().includes(q)
  )
})

function selectTool(tool: McpTool) {
  selectedTool.value = tool
  lastResult.value = null
  paramValues.value = {}
  tool.parameters.forEach(p => {
    if (p.name === 'protocol') paramValues.value[p.name] = 'http'
    if (p.name === 'limit') paramValues.value[p.name] = '20'
  })
}

async function executeTool() {
  if (!selectedTool.value) return

  for (const p of selectedTool.value.parameters) {
    if (p.required && !paramValues.value[p.name]) {
      message.warning(`参数 ${p.name} 是必填项`)
      return
    }
  }

  executing.value = true
  const tool = selectedTool.value.name
  let result: any
  let isError = false

  try {
    switch (tool) {
      case 'get_proxy':
        result = await fetchRandomProxy(protocolParam())
        break
      case 'get_best_proxy':
        result = await fetchBestProxy(protocolParam())
        break
      case 'list_proxies':
        result = await fetchProxies(protocolParam(), numberParam('limit', 20))
        break
      case 'service_status':
      case 'pool_status':
      case 'proxy_stats':
        result = await fetchStatus()
        break
      case 'warp_status':
        result = await fetchWarpInstances()
        break
      case 'remove_proxy':
        if (!window.confirm('确认从代理池移除该代理？')) {
          result = { status: 'cancelled' }
          break
        }
        await deleteProxy(protocolParam(), paramValues.value.host, numberParam('port', 0))
        result = { status: 'ok' }
        break
      case 'refresh_pool':
        await refreshPool()
        result = { status: 'scheduled' }
        break
      case 'fetcher_status':
        result = await fetchFetcherStatus()
        break
      case 'refresh_fetcher':
        result = await refreshFetcher(paramValues.value.fetcher)
        break
      case 'route_test':
        result = await testRoute(paramValues.value.host, protocolParam())
        break
      case 'explain_proxy_scores':
        result = await fetchScoredProxies(protocolParam(), numberParam('limit', 20))
        break
      case 'check_proxy':
      case 'geoip_lookup':
      case 'cleanup_low_score_proxies':
      case 'update_service':
        result = mcpTransportRequired(tool)
        isError = true
        break
      default:
        result = { error: `Unknown tool: ${tool}` }
        isError = true
    }

    recordResult(tool, result, isError)
  } catch (e: any) {
    recordResult(tool, e?.response?.data || { error: e?.message || '执行失败' }, true)
  } finally {
    executing.value = false
  }
}

function protocolParam(): Protocol {
  const value = paramValues.value.protocol || 'http'
  if (value === 'http' || value === 'https' || value === 'socks4' || value === 'socks5') {
    return value
  }
  return 'http'
}

function numberParam(name: string, fallback: number): number {
  const parsed = Number(paramValues.value[name])
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : fallback
}

function mcpTransportRequired(tool: string) {
  return {
    status: 'mcp_transport_required',
    tool,
    message: '该工具没有 REST 等效端点，需要通过 stdio 或 Streamable HTTP MCP transport 调用。',
  }
}

function recordResult(tool: string, result: any, isError: boolean) {
  const content = typeof result === 'string' ? result : JSON.stringify(result, null, 2)
  lastResult.value = { content, isError }
  callHistory.value.unshift({
    time: new Date().toTimeString().split(' ')[0],
    tool,
    params: JSON.stringify(paramValues.value),
    result: content.substring(0, 200),
    isError,
  })
}

function formatResult(content: string): string {
  try {
    return JSON.stringify(JSON.parse(content), null, 2)
  } catch {
    return content
  }
}

const historyColumns = [
  { title: '时间', key: 'time', width: 100 },
  { title: '工具', key: 'tool', width: 160 },
  { title: '参数', key: 'params', width: 220, ellipsis: { tooltip: true } },
  {
    title: '状态',
    key: 'isError',
    width: 80,
    render: (row: any) =>
      h(NTag, { size: 'small', type: row.isError ? 'error' : 'success' }, { default: () => row.isError ? '失败' : '成功' }),
  },
  { title: '结果', key: 'result', ellipsis: { tooltip: true } },
]
</script>

<style scoped>
.search-box {
  margin-bottom: 12px;
}

.tool-card {
  cursor: pointer;
}

.tool-active {
  border-color: #63e2b7 !important;
  background: rgba(99, 226, 183, 0.08) !important;
}

.tool-name {
  font-weight: 600;
  font-size: 14px;
}

.tool-description {
  color: #8f8f9d;
  font-size: 12px;
  margin-top: 4px;
}

.required-tag {
  margin-left: 4px;
}

.result-header {
  display: flex;
  justify-content: space-between;
  margin-bottom: 8px;
}

.result-title {
  font-weight: 600;
}

.result-code {
  max-height: 400px;
  overflow: auto;
}
</style>
