<template>
  <n-space vertical :size="16">
    <n-card title="实时日志">
      <n-empty description="Web 日志流暂未接入">
        <template #extra>
          <n-space>
            <n-button @click="$router.push('/')">查看服务状态</n-button>
            <n-button @click="$router.push('/fetchers')">查看抓取源</n-button>
            <n-button @click="$router.push('/routes')">路由 Dry-run</n-button>
          </n-space>
        </template>
      </n-empty>
    </n-card>

    <n-card title="当前可用的真实排障入口">
      <n-list>
        <n-list-item>
          <n-thing title="/api/status">
            服务版本、git hash、运行时间、代理池、Redis、WARP 和 xray 摘要。
          </n-thing>
        </n-list-item>
        <n-list-item>
          <n-thing title="/api/readyz">
            Redis 等依赖 readiness；适合判断服务是否可以处理真实请求。
          </n-thing>
        </n-list-item>
        <n-list-item>
          <n-thing title="/api/fetchers">
            抓取源最近运行状态、耗时、抓取数量和错误原因。
          </n-thing>
        </n-list-item>
        <n-list-item>
          <n-thing title="/api/routes/test">
            路由规则、GeoIP、候选出口和 fallback 诊断。
          </n-thing>
        </n-list-item>
      </n-list>
    </n-card>
  </n-space>
</template>
