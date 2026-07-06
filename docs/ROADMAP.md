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
| Paused Closeout | 已有较大进展或代码已落地，但当前按用户要求暂不继续收尾 |
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

## Current Planning Decision

当前已按用户要求暂停继续推进 `gateway-route-debugging` 的发布后收尾，并完成第一轮 Roadmap / Trellis 状态清理。下一项正式推进 `no-ssh-dev-validation`，先把不依赖直接 SSH 的 dev 验证闭环固化，再进入代理评分与保留策略。

**工作区注意事项**：

- 当前本地存在一组已隔离的 `fetcher-validator-quality` WIP：`stash@{0}: wip: paused fetcher circuit work`。不要默认恢复、删除或混入后续任务。
- 当前 Trellis 里 `gateway-route-debugging` 和 `fetcher-validator-quality` 已从 `in_progress` 改为 `paused`，当前会话任务指针已清空。
- `.codex/config.toml` 属于非本任务改动，不纳入任何 roadmap 提交或后续功能提交。
- 按用户要求，不直接 SSH 到 dev 地址；dev 验证默认走 HTTP、MCP、GitHub Actions、容器已有自更新入口和公开状态接口。

## Now

### P0 — `no-ssh-dev-validation`

**目标**：形成不依赖直接 SSH 的 dev 验证闭环，避免部署和故障验证依赖人工登录服务器。

**为什么先做**：用户已经明确禁止直接 SSH 到 dev 地址。后续每个发布、冒烟和故障验证任务都依赖这条规则，先固化流程可以避免后面重复犹豫或误用服务器登录。

**建议范围**：

- [ ] 明确 dev 验证只使用 HTTP、MCP、GitHub Actions、容器内已有 update_service 和公开状态接口。
- [ ] 清理或隔离测试 helper 中“需要 SSH / 直接 Docker API”的假设。
- [ ] 补充一份可重复的 dev-only 验证步骤：构建、推送、等待 Actions、触发更新、验证 `/api/status.git_hash`、验证 MCP。
- [ ] 把不能自动化的故障注入项标记为手工/延后，并说明风险。

**验收标准**：

- [ ] 不使用 SSH 即可完成一次发布后冒烟验证。
- [ ] 相关测试 helper 不再把 SSH 作为默认路径。
- [ ] 文档说明 dev 验证步骤和禁止事项。
- [ ] `python -m py_compile tests/integration/**/*.py` 或等价 Python 检查通过。

## Paused Closeout

### P1 — `gateway-route-debugging`

**目标**：让网关路由决策和 fallback 链路可解释、可观测、可测试。

**当前状态**：核心实现已落地并推送到 `2842043 feat: add gateway route diagnostics`，包括 route dry-run、MCP `route_test`、gateway fallback 尝试指标和本地测试验证。用户已要求“先不做这个”，因此暂不继续发布后文档/归档收尾。

**已完成并验证**：

- [x] 为网关请求记录 route rule、GeoIP 结果、出口选择、fallback 候选和最终选择。
- [x] 新增 route dry-run 能力：输入 host/protocol，返回命中规则、GeoIP、出口和 fallback 顺序。
- [x] MCP 增加 `route_test` 工具。
- [x] 对 gateway route attempts 增加 Prometheus 指标。
- [x] 增加 gateway / API / MCP / core 相关自动化测试。
- [x] `cargo test --workspace --all-targets` 通过。
- [x] `cargo clippy --workspace --all-targets -- -D warnings` 通过。

**暂缓 TODO**：

- [ ] 按用户确认后再做 Trellis 任务归档和最终文档收尾。
- [ ] 可选 debug header，仅在配置启用时返回路由诊断信息。

## Done

### P0 — `todo-queue-and-task-state-cleanup`

**目标**：让 Roadmap 和 Trellis 任务状态重新成为可信的执行入口，后续每次只推进一个明确任务。

**当前状态**：已完成本地清理并提交第一版新 Roadmap；旧任务未归档、未恢复 stash，只移除“正在做”的信号。

**主要完成项**：

- [x] Roadmap 新增 `Paused Closeout` 状态。
- [x] `gateway-route-debugging` 从 Now 移到 `Paused Closeout`，发布后文档/归档收尾按用户要求暂缓。
- [x] `fetcher-validator-quality` 保持暂缓，WIP 继续隔离在 `stash@{0}`。
- [x] Trellis 中 `gateway-route-debugging` 和 `fetcher-validator-quality` 的状态从 `in_progress` 改为 `paused`。
- [x] 当前 Trellis 会话任务指针已清空，`task.py current --source` 返回 none。
- [x] 下一项正式开发任务明确为 `no-ssh-dev-validation`。

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

**验收标准**：

- [ ] 文档明确当前 score 公式、降权规则和清理规则。
- [ ] API/MCP 至少一个运维入口能返回 score explain。
- [ ] 失败、过期和低分代理的保留/降权/清理行为有自动化测试覆盖。
- [ ] `cargo test --workspace --all-targets` 通过。
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 通过。

## Next

### P1 — `validator-observability-v2`

**目标**：进一步提升 `check_proxy` 和批量验证结果的解释能力，说明代理“能连哪里、慢在哪里、出口是谁”。

**候选功能**：

- [ ] 验证结果记录 TCP 连接时间和请求耗时。
- [ ] 验证结果记录出口 IP、国家/地区。
- [ ] 验证目标支持多 URL：默认目标、国内目标、国外目标、Cloudflare trace。
- [ ] MCP `check_proxy` 返回多目标检查结果和稳定错误分类。

### P2 — `web-dashboard-real-ops-mvp`

**目标**：把 Web Dashboard 从演示面板推进为真实可用的运维入口。

**候选功能**：

- [ ] 首页总览使用真实 `/api/status`、`/api/metrics` 或新增摘要接口。
- [ ] MCP Debug 工具列表同步后端真实工具，包括 `service_status`、`fetcher_status`、`refresh_fetcher`、`update_service` 和后续 `route_test`。
- [ ] Logs 页面移除模拟数据，改为真实 API / SSE / WebSocket 方案，或在后端能力未完成前隐藏该入口。
- [ ] 抓取源页面展示源状态、手动刷新、错误历史。
- [ ] 路由调试页面对接 `route_test`。

## Later

### P1 — `fetcher-validator-quality`

**目标**：继续提升代理来源质量、验证结果可解释性和错误诊断能力。

**当前状态**：Trellis 任务已存在：`.trellis/tasks/07-07-fetcher-validator-quality/`。第一批实现已覆盖 fetcher 运行报告、单源刷新、API/MCP 运维入口和 `check_proxy` 结构化错误；剩余内部增强暂缓，等待当前 WIP 被隔离或重新确认后再恢复。

**已完成并验证**：

- [x] 为每个 fetcher 记录最近抓取时间、成功/失败状态、抓取数量、解析数量和错误原因。
- [x] 支持按 fetcher 手动刷新，避免每次全量刷新。
- [x] 验证错误分类：invalid proxy URL、client build failed、timeout、request failed、bad status、body read failed。
- [x] MCP `check_proxy` 返回结构化错误类型。
- [x] `cargo fmt --all --check`
- [x] `cargo test --workspace --all-targets`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`

**暂缓 TODO**：

- [ ] 增加源级熔断：连续失败后暂停，恢复时半开探测。
- [ ] 验证结果记录 TCP 连接时间、请求耗时、出口 IP、国家/地区。
- [ ] 验证目标支持多 URL：默认目标、国内目标、国外目标、Cloudflare trace。

### P0 — `update-failure-hardening`

**目标**：在不影响正常发布节奏的前提下，补齐自更新失败路径的故障注入验证。

**候选功能**：

- [ ] 错误镜像 tag / digest 时，`update_service` 返回结构化错误，旧容器继续运行。
- [ ] 错误 Watchtower token 时，`update_service` 返回结构化错误，旧容器继续运行。
- [ ] Watchtower HTTP endpoint 不可达时，`update_service` 返回结构化错误，旧容器继续运行。
- [ ] 形成一份可重复执行的 dev-only 验证步骤，避免误操作生产配置。
- [ ] 必要时增加自动化集成测试或最小脚本化检查。

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

1. `no-ssh-dev-validation` — 固化无 SSH 的 dev 验证闭环。
2. `score-retention-policy` — 基于现有验证结果做评分、降权和清理。
3. `validator-observability-v2` — 多目标验证、出口 IP 和耗时拆分。
4. `web-dashboard-real-ops-mvp` — 管理面板接入真实运维数据。
5. `gateway-route-debugging` — 用户确认后再做任务归档、最终文档收尾或可选 debug header。
6. `fetcher-validator-quality` — 用户确认后恢复暂缓的源级熔断等内部增强。
7. `update-failure-hardening` — 仅在允许故障注入 dev 配置且不需要 SSH 时执行。
8. `warp-ops-enhancement` — WARP 运维增强。
9. `xray-subscription-ops` — xray 和订阅源管理。

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
