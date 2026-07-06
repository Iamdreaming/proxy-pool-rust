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

暂无进行中的代码任务。当前建议下一步创建并启动 `fetcher-validator-quality`，因为部署闭环和基础观测入口已经具备，下一瓶颈是代理输入质量、验证解释性和错误诊断能力。

## Done

### P0 — `ci-mcp-auto-update`

**目标**：打通“本地改代码 → git push → GitHub Actions 构建镜像 → MCP 触发服务器拉取新镜像并重启 → 线上验证 git hash”的完整闭环。

**当前状态**：主链路已完成并在 dev 验证。失败注入验证按用户要求暂缓，后续单独拆为 `update-failure-hardening`。

**主要完成项**：

- [x] GitHub Actions 已成功构建并推送 GHCR 镜像。
- [x] `docker-compose.yml` 改用 GHCR 镜像，并保留本地 build 参考。
- [x] MCP 增加 `update_service` 工具，并通过显式环境变量安全开关启用。
- [x] `/api/status` 增加 `version` 和 `git_hash`。
- [x] 更新前后返回镜像 ID / digest 对比。
- [x] 本地通过 `cargo test`、`cargo clippy -- -D warnings` 和 `npm run build`。
- [x] 推送后监控 `docker-build.yml` GitHub Actions。
- [x] 通过 MCP 更新 dev 服务，并验证 `/api/status.git_hash=0b6f919`。
- [x] dev 容器已同步 `PROXY_POOL_UPDATE_*` 和 Watchtower token，MCP `update_service` 返回 `already_current`。
- [ ] （暂缓）更新失败时旧容器继续运行的失败注入验证。

### P0 — `status-health-observability`

**目标**：让服务状态、版本、依赖和基础后台任务可观测，为后续代理池优化提供可靠诊断入口。

**当前状态**：基础观测能力已落地；后续只保留增量增强，不再作为下一轮主任务重复实现。

**主要完成项**：

- [x] `/api/status` 返回 app version、git hash、uptime、pool 摘要、Redis 状态、WARP/xray 摘要。
- [x] `/api/healthz` 只检查进程存活，适合容器健康检查。
- [x] `/api/readyz` 返回结构化依赖 readiness，并能用 HTTP 503 表示 Redis 不可用。
- [x] MCP `service_status` 返回与 API 状态一致的结构化信息。
- [x] `/api/metrics` 暴露基础 Prometheus metrics：pool size、Redis readiness、WARP/xray 状态、uptime。
- [x] 单元测试和集成测试覆盖状态结构、healthz、readyz、service_status。

## Ready

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

**非目标**：

- 不重写抓取器框架。
- 不引入完整 Dashboard。
- 不改变网关路由策略。
- 不做大规模 Redis schema 破坏性迁移。

**建议验收标准**：

1. fetcher 状态能说明最近一次抓取是否成功、抓到多少、解析多少、失败原因是什么。
2. 可以只刷新指定 fetcher，不必全量刷新所有来源。
3. 连续失败的来源会进入暂停/半开探测状态，避免无意义重试。
4. 验证结果包含延迟、出口 IP、匿名度、地区和结构化错误类型。
5. MCP `check_proxy` 的失败结果可被程序稳定解析。
6. `cargo test` 和 `cargo clippy -- -D warnings` 通过。

## Next

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

### P0 — `update-failure-hardening`

**目标**：在不影响正常发布节奏的前提下，补齐自更新失败路径的故障注入验证。

**候选功能**：

- [ ] 错误镜像 tag / digest 时，`update_service` 返回结构化错误，旧容器继续运行。
- [ ] 错误 Watchtower token 时，`update_service` 返回结构化错误，旧容器继续运行。
- [ ] Watchtower HTTP endpoint 不可达时，`update_service` 返回结构化错误，旧容器继续运行。
- [ ] 形成一份可重复执行的 dev-only 验证步骤，避免误操作生产配置。
- [ ] 必要时增加自动化集成测试或最小脚本化检查。

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

1. `fetcher-validator-quality` — 推荐下一步，先提高代理输入质量。
2. `gateway-route-debugging` — 路由和 fallback 可诊断性。
3. `score-retention-policy` — 基于更完整的验证结果做评分、降权和清理。
4. `mcp-ops-tooling` — 运维工具补全。
5. `update-failure-hardening` — 仅在允许故障注入 dev 配置时执行。
6. `warp-ops-enhancement` — WARP 运维增强。
7. `xray-subscription-ops` — xray 和订阅源管理。
8. `web-dashboard-mvp` — 管理面板 MVP。

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
