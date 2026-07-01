<template>
  <n-space vertical :size="16">
    <!-- Tool list + executor -->
    <n-grid :cols="3" :x-gap="16">
      <!-- Left: Tool list -->
      <n-gi :span="1">
        <n-card title="MCP Tools" style="height: 100%">
          <n-input v-model:value="toolSearch" placeholder="搜索工具..." clearable style="margin-bottom: 12px" />
          <n-space vertical :size="8">
            <n-card
              v-for="tool in filteredTools"
              :key="tool.name"
              size="small"
              :class="{ 'tool-card': true, 'tool-active': selectedTool?.name === tool.name }"
              hoverable
              @click="selectTool(tool)"
              style="cursor: pointer"
            >
              <div style="font-weight: 600; font-size: 14px">{{ tool.name }}</div>
              <div style="color: #aaa; font-size: 12px; margin-top: 4px">{{ tool.description }}</div>
            </n-card>
          </n-space>
        </n-card>
      </n-gi>

      <!-- Right: Tool executor -->
      <n-gi :span="2">
        <n-card v-if="selectedTool" :title="`调用: ${selectedTool.name}`" style="height: 100%">
          <template #header-extra>
            <n-button type="primary" @click="executeTool" :loading="executing">
              ▶ 执行
            </n-button>
          </template>

          <n-space vertical :size="16">
            <!-- Tool description -->
            <n-alert :bordered="false" type="info">
              {{ selectedTool.description }}
            </n-alert>

            <!-- Parameters form -->
            <n-form v-if="selectedTool.parameters.length > 0" label-placement="left" label-width="120">
              <n-form-item
                v-for="param in selectedTool.parameters"
                :key="param.name"
                :label="param.name"
              >
                <n-input
                  v-model:value="paramValues[param.name]"
                  :placeholder="param.description || `${param.type}`"
                  :type="param.type === 'number' ? 'text' : 'text'"
                />
                <template #label>
                  <span>{{ param.name }}</span>
                  <n-tag v-if="param.required" size="tiny" type="error" style="margin-left: 4px">必填</n-tag>
                </template>
              </n-form-item>
            </n-form>

            <n-divider v-if="lastResult" />

            <!-- Result display -->
            <div v-if="lastResult">
              <div style="display: flex; justify-content: space-between; margin-bottom: 8px">
                <span style="font-weight: 600">返回结果</span>
                <n-tag :type="lastResult.isError ? 'error' : 'success'" size="small">
                  {{ lastResult.isError ? '❌ 错误' : '✅ 成功' }}
                </n-tag>
              </div>
              <n-code
                :code="formatResult(lastResult.content)"
                language="json"
                :word-wrap="true"
                style="max-height: 400px; overflow: auto"
              />
            </div>
          </n-space>
        </n-card>

        <n-card v-else title="MCP 调试面板" style="height: 100%">
          <n-space vertical align="center" :size="16" style="padding: 40px 0">
            <span style="font-size: 64px">🤖</span>
            <span style="color: #aaa">选择左侧工具开始调试</span>
          </n-space>
        </n-card>
      </n-gi>
    </n-grid>

    <!-- Call history -->
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
import { ref, computed, h } from 'vue'
import { NTag, useMessage } from 'naive-ui'
import type { McpTool, McpCallResult } from '@/types'

const message = useMessage()
const toolSearch = ref('')
const selectedTool = ref<McpTool | null>(null)
const paramValues = ref<Record<string, string>>({})
const executing = ref(false)
const lastResult = ref<McpCallResult | null>(null)
const callHistory = ref<Array<{ time: string; tool: string; params: string; result: string; isError: boolean }>>([])

// MCP tool definitions matching the Rust backend
const mcpTools: McpTool[] = [
  {
    name: 'get_proxy',
    description: '获取一个可用代理（随机）',
    parameters: [
      { name: 'protocol', type: 'string', description: '协议: http, https, socks4, socks5', required: false },
    ],
  },
  {
    name: 'get_best_proxy',
    description: '获取评分最高的代理',
    parameters: [
      { name: 'protocol', type: 'string', description: '协议: http, https, socks4, socks5', required: false },
    ],
  },
  {
    name: 'list_proxies',
    description: '列出池中的代理',
    parameters: [
      { name: 'protocol', type: 'string', description: '协议过滤', required: false },
      { name: 'limit', type: 'number', description: '返回数量 (默认20)', required: false },
    ],
  },
  {
    name: 'check_proxy',
    description: '验证指定代理的可用性',
    parameters: [
      { name: 'host', type: 'string', description: '代理 IP 地址', required: true },
      { name: 'port', type: 'number', description: '代理端口', required: true },
      { name: 'protocol', type: 'string', description: '代理协议', required: true },
    ],
  },
  {
    name: 'pool_status',
    description: '查看代理池状态概览',
    parameters: [],
  },
  {
    name: 'warp_status',
    description: '查看 WARP 实例状态',
    parameters: [],
  },
  {
    name: 'geoip_lookup',
    description: '查询主机地理位置',
    parameters: [
      { name: 'host', type: 'string', description: '主机名或 IP', required: true },
    ],
  },
  {
    name: 'remove_proxy',
    description: '从池中移除代理',
    parameters: [
      { name: 'host', type: 'string', description: '代理 IP', required: true },
      { name: 'port', type: 'number', description: '代理端口', required: true },
      { name: 'protocol', type: 'string', description: '代理协议', required: true },
    ],
  },
  {
    name: 'refresh_pool',
    description: '触发代理池刷新（抓取+验证）',
    parameters: [],
  },
  {
    name: 'proxy_stats',
    description: '查看代理池统计信息',
    parameters: [],
  },
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
  // Pre-fill defaults
  tool.parameters.forEach(p => {
    if (p.name === 'protocol') paramValues.value[p.name] = 'http'
  })
}

async function executeTool() {
  if (!selectedTool.value) return

  // Validate required params
  for (const p of selectedTool.value.parameters) {
    if (p.required && !paramValues.value[p.name]) {
      message.warning(`参数 ${p.name} 是必填项`)
      return
    }
  }

  executing.value = true
  try {
    // Map tool name to API endpoint
    const tool = selectedTool.value.name
    let result: any

    switch (tool) {
      case 'get_proxy': {
        const protocol = paramValues.value.protocol || 'http'
        const resp = await fetch(`/api/proxy/random?protocol=${protocol}`)
        result = await resp.json()
        break
      }
      case 'get_best_proxy': {
        const protocol = paramValues.value.protocol || 'http'
        const resp = await fetch(`/api/proxy/best?protocol=${protocol}`)
        result = await resp.json()
        break
      }
      case 'list_proxies': {
        const protocol = paramValues.value.protocol || 'http'
        const limit = paramValues.value.limit || '20'
        const resp = await fetch(`/api/proxies?protocol=${protocol}&limit=${limit}`)
        result = await resp.json()
        break
      }
      case 'check_proxy': {
        // Use the validator endpoint (TODO: add dedicated API)
        result = { message: '代理验证功能需通过 MCP 协议调用', hint: '请使用 stdio/HTTP MCP 传输' }
        break
      }
      case 'pool_status': {
        const resp = await fetch('/api/status')
        result = await resp.json()
        break
      }
      case 'warp_status': {
        const resp = await fetch('/api/warp')
        result = await resp.json()
        break
      }
      case 'refresh_pool': {
        await fetch('/api/proxies/refresh', { method: 'POST' })
        result = { status: 'scheduled' }
        break
      }
      case 'proxy_stats': {
        const resp = await fetch('/api/status')
        result = await resp.json()
        break
      }
      default:
        result = { error: `Unknown tool: ${tool}` }
    }

    const content = typeof result === 'string' ? result : JSON.stringify(result, null, 2)
    lastResult.value = { content, isError: false }

    // Add to history
    callHistory.value.unshift({
      time: new Date().toTimeString().split(' ')[0],
      tool,
      params: JSON.stringify(paramValues.value),
      result: content.substring(0, 200),
      isError: false,
    })
  } catch (e: any) {
    lastResult.value = { content: e.message || '执行失败', isError: true }
    callHistory.value.unshift({
      time: new Date().toTimeString().split(' ')[0],
      tool: selectedTool.value.name,
      params: JSON.stringify(paramValues.value),
      result: e.message || '执行失败',
      isError: true,
    })
  } finally {
    executing.value = false
  }
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
  { title: '工具', key: 'tool', width: 140 },
  { title: '参数', key: 'params', width: 200, ellipsis: { tooltip: true } },
  {
    title: '状态', key: 'isError', width: 80,
    render: (row: any) => h(NTag, { size: 'small', type: row.isError ? 'error' : 'success' }, { default: () => row.isError ? '失败' : '成功' }),
  },
  { title: '结果', key: 'result', ellipsis: { tooltip: true } },
]
</script>

<style scoped>
.tool-active {
  border-color: #63e2b7 !important;
  background: rgba(99, 226, 183, 0.08) !important;
}
</style>
