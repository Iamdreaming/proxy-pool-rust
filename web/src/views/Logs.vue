<template>
  <n-space vertical :size="16">
    <n-card title="实时日志">
      <template #header-extra>
        <n-space>
          <n-switch v-model:value="autoScroll" />
          <span style="color: #aaa; font-size: 13px">自动滚动</span>
          <n-button size="small" @click="clearLogs">清空</n-button>
        </n-space>
      </template>

      <div
        ref="logContainer"
        style="background: #1a1a1e; border-radius: 6px; padding: 12px; font-family: monospace; font-size: 13px; height: 600px; overflow-y: auto;"
      >
        <div v-for="(log, i) in logs" :key="i" style="margin-bottom: 2px;">
          <span :style="{ color: levelColor(log.level) }">[{{ log.level }}]</span>
          <span style="color: #666; margin: 0 8px">{{ log.time }}</span>
          <span style="color: #ccc">{{ log.message }}</span>
        </div>
        <div v-if="logs.length === 0" style="color: #555; text-align: center; padding: 40px">
          等待日志...（WebSocket 连接开发中，当前显示模拟数据）
        </div>
      </div>
    </n-card>
  </n-space>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted, nextTick } from 'vue'

interface LogEntry {
  level: string
  time: string
  message: string
}

const logs = ref<LogEntry[]>([])
const autoScroll = ref(true)
const logContainer = ref<HTMLElement | null>(null)

function levelColor(level: string): string {
  switch (level) {
    case 'ERROR': return '#e88080'
    case 'WARN': return '#f2c97d'
    case 'INFO': return '#63e2b7'
    case 'DEBUG': return '#666'
    default: return '#aaa'
  }
}

function addLog(level: string, message: string) {
  const now = new Date()
  const time = now.toTimeString().split(' ')[0]
  logs.value.push({ level, time, message })
  if (logs.value.length > 1000) {
    logs.value = logs.value.slice(-800)
  }
  if (autoScroll.value) {
    nextTick(() => {
      if (logContainer.value) {
        logContainer.value.scrollTop = logContainer.value.scrollHeight
      }
    })
  }
}

function clearLogs() {
  logs.value = []
}

// TODO: Replace with real WebSocket connection
// const ws = new WebSocket(`ws://${location.host}/api/logs/ws`)
// ws.onmessage = (event) => { addLog('INFO', event.data) }

// Simulated logs for demo
let demoTimer: ReturnType<typeof setInterval> | null = null

onMounted(() => {
  addLog('INFO', 'Proxy pool server started')
  addLog('INFO', 'Scheduler initialized: fetch_interval=300s, validate_interval=60s')
  addLog('INFO', 'Gateway listening on 0.0.0.0:9080')
  addLog('INFO', 'API server listening on 0.0.0.0:8000')
  addLog('INFO', 'MCP server started (stdio transport)')

  demoTimer = setInterval(() => {
    const messages = [
      ['INFO', 'ProxyScrape: fetched 120 proxies (98 unique)'],
      ['INFO', 'FreeProxyList: fetched 45 proxies (32 unique)'],
      ['INFO', 'Validated 87 working proxies'],
      ['WARN', 'validate 103.152.112.166:8080 failed: connection timeout'],
      ['INFO', 'GeoNode: fetched 200 proxies (180 unique)'],
      ['DEBUG', 'Circuit breaker reset for 45.33.32.156:80'],
      ['INFO', 'WARP health check: 3/3 instances healthy'],
    ]
    const [level, msg] = messages[Math.floor(Math.random() * messages.length)]
    addLog(level as string, msg as string)
  }, 3000)
})

onUnmounted(() => {
  if (demoTimer) clearInterval(demoTimer)
})
</script>
