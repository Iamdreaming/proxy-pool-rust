import { createRouter, createWebHistory } from 'vue-router'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      component: () => import('@/layouts/MainLayout.vue'),
      children: [
        { path: '', name: 'dashboard', component: () => import('@/views/Dashboard.vue') },
        { path: 'proxies', name: 'proxies', component: () => import('@/views/Proxies.vue') },
        { path: 'warp', name: 'warp', component: () => import('@/views/Warp.vue') },
        { path: 'routes', name: 'routes', component: () => import('@/views/Routes.vue') },
        { path: 'logs', name: 'logs', component: () => import('@/views/Logs.vue') },
        { path: 'mcp', name: 'mcp', component: () => import('@/views/McpDebug.vue') },
        { path: 'settings', name: 'settings', component: () => import('@/views/Settings.vue') },
      ],
    },
  ],
})

export default router
