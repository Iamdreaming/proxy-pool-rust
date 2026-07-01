<template>
  <n-space vertical :size="16">
    <n-card title="系统设置">
      <n-alert type="info" :bordered="false" style="margin-bottom: 16px">
        修改配置后需要重启服务才能生效。配置文件路径：config/settings.yaml
      </n-alert>

      <n-form label-placement="left" label-width="160" disabled>
        <n-h4>网关</n-h4>
        <n-form-item label="监听地址">
          <n-input :value="settings.gateway?.listen_host || '0.0.0.0'" />
        </n-form-item>
        <n-form-item label="监听端口">
          <n-input :value="String(settings.gateway?.listen_port || 9080)" />
        </n-form-item>

        <n-h4>API</n-h4>
        <n-form-item label="监听地址">
          <n-input :value="settings.api?.listen_host || '0.0.0.0'" />
        </n-form-item>
        <n-form-item label="监听端口">
          <n-input :value="String(settings.api?.listen_port || 8000)" />
        </n-form-item>

        <n-h4>MCP</n-h4>
        <n-form-item label="传输方式">
          <n-input :value="settings.mcp?.transport || 'both'" />
        </n-form-item>
        <n-form-item label="HTTP 端口">
          <n-input :value="String(settings.mcp?.http_port || 9000)" />
        </n-form-item>

        <n-h4>代理池</n-h4>
        <n-form-item label="抓取间隔(秒)">
          <n-input :value="String(settings.pool?.fetch_interval_sec || 300)" />
        </n-form-item>
        <n-form-item label="验证间隔(秒)">
          <n-input :value="String(settings.pool?.validate_interval_sec || 60)" />
        </n-form-item>
        <n-form-item label="验证并发数">
          <n-input :value="String(settings.pool?.validate_concurrency || 100)" />
        </n-form-item>
        <n-form-item label="验证超时(秒)">
          <n-input :value="String(settings.pool?.validate_timeout_sec || 10)" />
        </n-form-item>
        <n-form-item label="验证目标 URL">
          <n-input :value="settings.pool?.validate_target_url || 'https://httpbin.org/ip'" />
        </n-form-item>

        <n-h4>Redis</n-h4>
        <n-form-item label="连接 URL">
          <n-input :value="settings.redis?.url || 'redis://localhost:6379/0'" />
        </n-form-item>
      </n-form>
    </n-card>
  </n-space>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'

const settings = ref<Record<string, any>>({})

onMounted(async () => {
  try {
    const resp = await fetch('/api/settings')
    if (resp.ok) {
      settings.value = await resp.json()
    }
  } catch {
    // Settings endpoint not yet implemented — show defaults
  }
})
</script>
