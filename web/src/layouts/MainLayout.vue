<template>
  <n-layout has-sider style="height: 100vh">
    <n-layout-sider
      bordered
      collapse-mode="width"
      :collapsed-width="64"
      :width="220"
      :collapsed="collapsed"
      show-trigger
      @collapse="collapsed = true"
      @expand="collapsed = false"
    >
      <div class="logo">
        <span v-if="!collapsed">🔄 Proxy Pool</span>
        <span v-else>🔄</span>
      </div>
      <n-menu
        :collapsed="collapsed"
        :collapsed-width="64"
        :collapsed-icon-size="22"
        :options="menuOptions"
        :value="activeKey"
        @update:value="handleMenuClick"
      />
    </n-layout-sider>

    <n-layout>
      <n-layout-header bordered style="height: 56px; display: flex; align-items: center; padding: 0 24px; justify-content: space-between">
        <n-breadcrumb>
          <n-breadcrumb-item>{{ currentPageTitle }}</n-breadcrumb-item>
        </n-breadcrumb>
        <n-space>
          <n-tag :type="poolHealthy ? 'success' : 'error'" size="small">
            池: {{ totalProxies }}
          </n-tag>
          <n-button quaternary circle @click="refreshData">
            <template #icon>🔄</template>
          </n-button>
        </n-space>
      </n-layout-header>

      <n-layout-content content-style="padding: 24px;">
        <router-view />
      </n-layout-content>
    </n-layout>
  </n-layout>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, h } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { NIcon } from 'naive-ui'
import { usePoolStore } from '@/stores/pool'

const router = useRouter()
const route = useRoute()
const poolStore = usePoolStore()
const collapsed = ref(false)

const poolHealthy = computed(() => poolStore.totalProxies() > 0)
const totalProxies = computed(() => poolStore.totalProxies())

const activeKey = computed(() => route.name as string)

const currentPageTitle = computed(() => {
  const titles: Record<string, string> = {
    dashboard: '概览',
    proxies: '代理列表',
    warp: 'WARP 管理',
    routes: '路由规则',
    logs: '实时日志',
    mcp: 'MCP 调试',
    settings: '系统设置',
  }
  return titles[route.name as string] || '概览'
})

function renderIcon(emoji: string) {
  return () => h(NIcon, null, { default: () => h('span', { style: 'font-size: 18px' }, emoji) })
}

const menuOptions = [
  { label: '概览', key: 'dashboard', icon: renderIcon('📊') },
  { label: '代理列表', key: 'proxies', icon: renderIcon('🌐') },
  { label: 'WARP 管理', key: 'warp', icon: renderIcon('☁️') },
  { label: '路由规则', key: 'routes', icon: renderIcon('🔀') },
  { label: '实时日志', key: 'logs', icon: renderIcon('📋') },
  { label: 'MCP 调试', key: 'mcp', icon: renderIcon('🤖') },
  { label: '系统设置', key: 'settings', icon: renderIcon('⚙️') },
]

function handleMenuClick(key: string) {
  router.push({ name: key })
}

async function refreshData() {
  await poolStore.loadStatus()
}

onMounted(() => {
  poolStore.loadStatus()
})
</script>

<style scoped>
.logo {
  height: 56px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 18px;
  font-weight: 600;
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}
</style>
