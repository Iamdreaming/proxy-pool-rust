<template>
  <n-space vertical :size="16">
    <n-card title="系统设置">
      <n-empty description="Web 配置查看与编辑 API 暂未接入">
        <template #extra>
          <n-alert type="info" :bordered="false">
            当前服务配置仍以 config/settings.yaml 为准。需要变更监听端口、验证目标、抓取间隔或外部依赖时，请通过部署配置更新并重启服务。
          </n-alert>
        </template>
      </n-empty>
    </n-card>

    <n-card title="当前可通过 Web 查看">
      <n-list>
        <n-list-item>
          <n-thing title="服务状态">概览页展示版本、git hash、运行时间、Redis、WARP、xray 和代理池摘要。</n-thing>
        </n-list-item>
        <n-list-item>
          <n-thing title="抓取源">抓取源页面展示 fetcher 状态，并支持单源刷新。</n-thing>
        </n-list-item>
        <n-list-item>
          <n-thing title="路由诊断">路由页面支持针对 host/protocol 的 dry-run。</n-thing>
        </n-list-item>
      </n-list>
    </n-card>
  </n-space>
</template>
