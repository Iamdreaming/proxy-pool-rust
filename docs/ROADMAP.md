# proxy-pool-rust Roadmap

> 本文档是项目级功能路线图，负责记录长期方向、优先级和可拆分任务。具体实现细节、验收标准和执行步骤应落到 `.trellis/tasks/` 中的独立任务。

## 管理原则

1. **Roadmap 管方向**：只记录优先级、范围和任务拆分，不替代 PRD / design / implement 文档。
2. **Trellis 管执行**：进入开发前，为可独立验收的功能创建 `.trellis/tasks/<task>/prd.md`。
3. **一次只做一个 In Progress**：避免多个大功能并行导致上下文漂移。
4. **每个任务必须有验收标准**：没有验收标准的 TODO 先留在 Parking Lot。
5. **完成后更新本文档**：每完成或取消一个任务，都同步调整状态。

## 状态定义

| 状态 | 含义 |
|------|------|
| Now | 当前正在做，最多 1 个 |
| Ready | PRD 清楚、验收标准明确，可以排队开工 |
| Next | 优先级较高，但还需要细化 PRD |
| Later | 后续增强，不阻塞近期迭代 |
| Parking Lot | 想法池，暂不承诺实现 |
| Done | 已完成并验证 |

## 优先级定义

| 优先级 | 含义 | 示例 |
|--------|------|------|
| P0 | 阻塞后续迭代或部署闭环 | CI/CD、自更新、版本信息、健康检查 |
| P1 | 核心代理池质量 | 抓取、验证、评分、熔断、fallback |
| P2 | 可观测性和运维效率 | metrics、MCP 运维工具、route dry-run、Dashboard MVP |
| P3 | 能力扩展 | WARP pinning、xray 生命周期、高级订阅管理 |

## Now

### P0 — `ci-mcp-auto-update`

**目标**：打通“本地改代码 → git push → GitHub Actions 构建镜像 → MCP 触发服务器拉取新镜像并重启 → 线上验证 git hash”的完整闭环。

**当前状态**：Trellis 任务已存在：`.trellis/tasks/07-03-ci-mcp-auto-update/`。

**主要 TODO**：

- [x] GitHub Actions workflow 已配置构建并推送 GHCR 镜像（远程运行待 push 验证）。
- [x] `docker-compose.yml` 改用 GHCR 镜像，并保留本地 build 参考。
- [x] MCP 增加 `update_service` 工具，并通过显式环境变量安全开关启用。
- [x] `/api/status` 增加 `version` 和 `git_hash`。
- [x] 更新前后返回镜像 ID / digest 对比。
- [ ] 更新失败时旧容器继续运行。
- [x] 本地通过 `cargo test` 和 `cargo clippy -- -D warnings`。
- [ ] 推送后监控 `docker-build.yml` GitHub Actions。
- [ ] 通过 MCP 更新线上服务，并验证 `/api/status.git_hash`。

**验收标准**：

1. `git push origin main` 后 GitHub Actions 成功构建并推送镜像到 GHCR。
2. MCP `update_service` 能拉取新镜像并重启容器。
3. 更新后 `/api/status` 显示新 git hash。
4. 更新失败时旧服务不中断。

## Ready

### P0 — `status-health-observability`

**目标**：让服务状态、版本、依赖和基础后台任务可观测，为后续代理池优化提供可靠诊断入口。

**建议范围**：

- [ ] 完善 `/api/status`：返回 app version、git hash、pool 摘要、Redis 状态、WARP/xray 摘要。
- [ ] 新增 `/api/healthz`：只检查进程存活，适合容器健康检查。
- [ ] 新增 `/api/readyz`：检查 Redis、核心配置和必要后台任务状态。
- [ ] MCP 增加 `service_status`：返回与 API 状态一致的结构化信息。
- [ ] 扩展基础 Prometheus metrics：pool size、验证计数、网关请求数、WARP/xray 状态。
- [ ] 为关键后台任务增加 tracing span。

**非目标**：

- 不实现完整 Web Dashboard。
- 不重构代理评分算法。
- 不改变网关路由策略。

**建议验收标准**：

1. `/api/healthz` 在服务进程存活时返回成功。
2. `/api/readyz` 能区分 Redis 不可用和服务自身不可用。
3. `/api/status` 能显示当前版本和核心组件摘要。
4. MCP `service_status` 与 API 状态信息一致。
5. `cargo test` 和 `cargo clippy -- -D warnings` 通过。

## Next

### P1 — `fetcher-validator-quality`

**目标**：提升代理来源质量、验证结果可解释性和错误诊断能力。

**建议范围**：

- [ ] 为每个 fetcher 记录最近抓取时间、成功/失败状态、抓取数量、解析数量和错误原因。
- [ ] 支持按 fetcher 手动刷新，避免每次全量刷新。
- [ ] 增加源级熔断：连续失败后暂停，恢复时半开探测。
- [ ] 验证结果记录 TCP 连接时间、请求耗时、总延迟、出口 IP、匿名度、国家/地区。
- [ ] 验证目标支持多 URL：默认目标、国内目标、国外目标、Cloudflare trace。
- [ ] 验证错误分类：timeout、connection refused、proxy auth error、invalid response、DNS error。
- [ ] MCP `check_proxy` 返回结构化错误类型。

### P1 — `gateway-route-debugging`

**目标**：让网关路由决策和 fallback 链路可解释、可观测、可测试。

**建议范围**：

- [ ] 为网关请求记录 route rule、GeoIP 结果、出口选择、fallback 次数和最终状态。
- [ ] 新增 route dry-run 能力：输入 host/url，返回预计命中规则、GeoIP、出口和 fallback 顺序。
- [ ] MCP 增加 `route_test` 工具。
- [ ] 对 `free_pool → WARP → xray → 502` 回退链增加 Prometheus 指标。
- [ ] 可选 debug header，仅在配置启用时返回路由诊断信息。
- [ ] 增加 gateway 集成测试：direct 成功、free_pool fallback、全部失败返回 502。

### P1 — `score-retention-policy`

**目标**：让代理评分、降权和清理策略更稳定、可解释。

**建议范围**：

- [ ] 明确当前 score 计算公式并写入文档。
- [ ] 返回 score 解释字段：latency、success rate、anonymity、penalty。
- [ ] 长时间未验证的代理降权。
- [ ] 多次失败代理快速降权。
- [ ] 支持按协议配置 `min_score`。
- [ ] 增加低质量代理自动清理任务。
- [ ] MCP 增加 `cleanup_low_score_proxies` 工具，并加安全开关。

## Later

### P2 — `mcp-ops-tooling`

**目标**：增强 LLM/运维侧对代理池的诊断和控制能力。

**候选功能**：

- [ ] `fetcher_status`：查询抓取源状态。
- [ ] `gateway_status`：查询网关运行状态和最近 fallback 摘要。
- [ ] `refresh_fetcher`：指定单个 fetcher 刷新。
- [ ] `route_test`：路由 dry-run。
- [ ] 危险工具配置开关：`update_service`、删除代理、清理代理、强制刷新。

### P2 — `web-dashboard-mvp`

**目标**：提供最小可用管理面板，用于查看服务状态、代理列表和基础运维动作。

**候选功能**：

- [ ] 首页总览：版本、git hash、pool 数量、可用率、最近刷新时间、WARP/xray 状态。
- [ ] 代理列表：协议筛选、分数排序、延迟排序、地区筛选、删除、手动验证。
- [ ] 抓取源页面：源状态、手动刷新、错误历史。
- [ ] 路由调试页面：输入目标域名，查看决策链。
- [ ] 部署页面：当前 git hash、镜像 digest、触发更新、更新日志。

### P3 — `warp-ops-enhancement`

**目标**：增强 WARP endpoint 优选、健康检查和手动运维能力。

**候选功能**：

- [ ] 完善 WARP instance 状态模型：endpoint、latency、loss、healthy、assigned_at、fail_count。
- [ ] 查询最近 WARP optimizer 扫描结果。
- [ ] 支持手动触发 WARP endpoint 扫描。
- [ ] 支持 endpoint pinning，允许临时禁用 optimizer 覆盖。
- [ ] WARP 健康检查结果进入 Prometheus metrics。
- [ ] 增加 WARP 链式代理端到端测试。

### P3 — `xray-subscription-ops`

**目标**：完善 xray 节点生命周期和订阅源运维能力。

**候选功能**：

- [ ] xray 节点生命周期：pending、activating、active、failed、removed。
- [ ] 记录每个节点激活失败原因。
- [ ] API/MCP 查询 active xray 节点摘要。
- [ ] 支持手动移除单个 xray 节点。
- [ ] 支持重新同步订阅节点到 xray。
- [ ] 增加 gRPC 重连状态指标。
- [ ] xray 配置变更时增加 dry-run 校验。
- [ ] 订阅源新增、删除、启用、禁用、手动刷新。
- [ ] 订阅解析结果预览和节点去重策略。

## Parking Lot

这些想法暂不承诺实现，等核心闭环稳定后再评估：

- [ ] 多区域出口调度。
- [ ] 代理质量历史趋势分析。
- [ ] Dashboard 高级图表。
- [ ] 基于国家/地区的出口偏好策略。
- [ ] 多租户或访问控制。
- [ ] 管理 API 鉴权。
- [ ] 自动回滚到上一镜像 digest。
- [ ] 外部告警集成，如 Telegram、Webhook、Prometheus Alertmanager。

## Trellis 任务创建建议

建议按以下顺序创建和推进任务：

1. `ci-mcp-auto-update` — 当前进行中。
2. `status-health-observability` — 下一个 Ready 任务。
3. `fetcher-validator-quality` — 代理输入质量。
4. `gateway-route-debugging` — 路由和 fallback 可诊断性。
5. `score-retention-policy` — 评分、降权和清理。
6. `mcp-ops-tooling` — 运维工具补全。
7. `warp-ops-enhancement` — WARP 运维增强。
8. `xray-subscription-ops` — xray 和订阅源管理。
9. `web-dashboard-mvp` — 管理面板 MVP。

## 任务 PRD 模板

```markdown
# PRD: <task-name>

## 背景

为什么要做这个任务。

## 目标

这个任务完成后，系统应该具备什么能力。

## 非目标

本次明确不做什么，防止范围膨胀。

## 需求

### F1: ...
### F2: ...
### F3: ...

## 验收标准

1. ...
2. ...
3. ...

## 验证方式

- cargo test
- cargo clippy -- -D warnings
- curl /api/status
- MCP tool 调用结果
```

## 维护节奏

每完成一个 Trellis 任务后：

1. 更新对应任务状态。
2. 更新本文档的 Now / Ready / Next / Later / Done。
3. 如有可复用规范，沉淀到 `.trellis/spec/`。
4. 同步 README 或配置示例中用户可见的功能说明。
5. 提交代码和文档。
6. 从 Ready / Next 中选择下一个任务。
