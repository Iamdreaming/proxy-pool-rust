<template>
  <n-space vertical :size="16">
    <n-card title="系统设置">
      <template #header-extra>
        <n-button size="small" :loading="loading" @click="loadSettings">刷新</n-button>
      </template>

      <n-alert v-if="error" type="error" :bordered="false" class="section-gap">
        {{ error }}
      </n-alert>
      <n-alert v-if="savedMessage" type="success" :bordered="false" class="section-gap">
        {{ savedMessage }}
      </n-alert>

      <n-spin :show="loading && !settingsMeta">
        <n-descriptions v-if="settingsMeta" bordered size="small" :column="2">
          <n-descriptions-item label="配置文件">
            <span class="path-text">{{ settingsMeta.path }}</span>
          </n-descriptions-item>
          <n-descriptions-item label="生效方式">
            <n-tag :type="settingsMeta.restart_required ? 'warning' : 'success'" size="small">
              {{ settingsMeta.restart_required ? '重启后生效' : '立即生效' }}
            </n-tag>
          </n-descriptions-item>
          <n-descriptions-item label="脱敏字段" :span="2">
            <n-space v-if="settingsMeta.redacted_fields.length" size="small">
              <n-tag
                v-for="field in settingsMeta.redacted_fields"
                :key="field"
                size="small"
                type="info"
              >
                {{ field }}
              </n-tag>
            </n-space>
            <span v-else>-</span>
          </n-descriptions-item>
        </n-descriptions>
      </n-spin>
    </n-card>

    <n-card title="配置内容">
      <n-input
        v-model:value="editor"
        type="textarea"
        :autosize="{ minRows: 22, maxRows: 36 }"
        :disabled="loading || saving"
        class="config-editor"
        placeholder="{}"
      />

      <template #action>
        <n-space justify="end">
          <n-button :disabled="loading || saving || !settingsMeta" @click="resetEditor">
            重置
          </n-button>
          <n-button
            type="primary"
            :loading="saving"
            :disabled="loading || !settingsMeta"
            @click="saveSettings"
          >
            保存配置
          </n-button>
        </n-space>
      </template>
    </n-card>
  </n-space>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useMessage } from 'naive-ui'
import { fetchSettings, updateSettings } from '@/api'
import type { ProxyPoolSettings, SettingsResponse } from '@/types'

const message = useMessage()
const loading = ref(false)
const saving = ref(false)
const error = ref('')
const savedMessage = ref('')
const settingsResponse = ref<SettingsResponse | null>(null)
const editor = ref('')

const settingsMeta = computed(() => settingsResponse.value)

function formatSettings(settings: ProxyPoolSettings): string {
  return JSON.stringify(settings, null, 2)
}

function errorMessage(e: any, fallback: string): string {
  return e?.response?.data?.status || e?.message || fallback
}

async function loadSettings() {
  loading.value = true
  error.value = ''
  savedMessage.value = ''
  try {
    const resp = await fetchSettings()
    settingsResponse.value = resp
    editor.value = formatSettings(resp.settings)
  } catch (e: any) {
    error.value = errorMessage(e, '加载配置失败')
    message.error('加载配置失败')
  } finally {
    loading.value = false
  }
}

function resetEditor() {
  if (!settingsResponse.value) return
  error.value = ''
  savedMessage.value = ''
  editor.value = formatSettings(settingsResponse.value.settings)
}

function parseEditor(): ProxyPoolSettings | null {
  try {
    const parsed = JSON.parse(editor.value)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
      error.value = '配置内容必须是 JSON 对象'
      return null
    }
    return parsed as ProxyPoolSettings
  } catch (e: any) {
    error.value = e?.message ? `JSON 解析失败：${e.message}` : 'JSON 解析失败'
    return null
  }
}

async function saveSettings() {
  const settings = parseEditor()
  if (!settings) {
    message.error('配置格式无效')
    return
  }

  saving.value = true
  error.value = ''
  savedMessage.value = ''
  try {
    const resp = await updateSettings(settings)
    settingsResponse.value = resp
    editor.value = formatSettings(resp.settings)
    savedMessage.value = resp.restart_required
      ? '配置已保存，重启服务后生效。'
      : '配置已保存。'
    message.success('配置已保存')
  } catch (e: any) {
    error.value = errorMessage(e, '保存配置失败')
    message.error('保存配置失败')
  } finally {
    saving.value = false
  }
}

onMounted(() => {
  loadSettings()
})
</script>

<style scoped>
.section-gap {
  margin-bottom: 12px;
}

.path-text {
  word-break: break-all;
}

.config-editor :deep(textarea) {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace;
  font-size: 13px;
  line-height: 1.5;
}
</style>
