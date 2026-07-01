<template>
  <n-space vertical :size="16">
    <n-card title="路由规则">
      <template #header-extra>
        <n-space>
          <n-button type="primary" @click="saveRoutes" :loading="saving">💾 保存</n-button>
          <n-button @click="addGroup">➕ 添加组</n-button>
        </n-space>
      </template>

      <n-space vertical :size="12">
        <n-alert type="info" :bordered="false">
          每个组列出域名后缀。裸域名如 github.com 匹配该主机和所有子域名。
          *.cn 匹配所有 .cn 结尾的主机。default 标记默认回退组。
        </n-alert>

        <div v-for="(entries, group) in groups" :key="group" style="margin-bottom: 12px">
          <n-card size="small" :title="String(group)">
            <template #header-extra>
              <n-button quaternary type="error" size="small" @click="removeGroup(String(group))">🗑️</n-button>
            </template>
            <n-dynamic-tags v-model:value="groups[String(group)]" />
          </n-card>
        </div>
      </n-space>
    </n-card>
  </n-space>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useMessage } from 'naive-ui'
import { fetchRoutes, updateRoutes } from '@/api'

const message = useMessage()
const groups = ref<Record<string, string[]>>({})
const saving = ref(false)

async function loadRoutes() {
  try {
    groups.value = await fetchRoutes()
  } catch {
    message.error('加载路由规则失败')
  }
}

async function saveRoutes() {
  saving.value = true
  try {
    await updateRoutes(groups.value)
    message.success('路由规则已保存并热重载')
  } catch {
    message.error('保存路由规则失败')
  } finally {
    saving.value = false
  }
}

function addGroup() {
  const name = `group_${Object.keys(groups.value).length + 1}`
  groups.value[name] = []
}

function removeGroup(name: string) {
  delete groups.value[name]
  // Trigger reactivity
  groups.value = { ...groups.value }
}

onMounted(() => {
  loadRoutes()
})
</script>
