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

当前已按用户要求暂不推进 `update-failure-hardening`，并已完成 `web-dashboard-real-ops-mvp`、`fetcher-source-circuit-breaker-mvp` 与 `validator-observability-v2` 的 single-target diagnostics MVP：Web Dashboard 现在优先展示真实运维数据或明确的不可用状态，抓取源具备源级熔断和手动探测能力，`check_proxy` 也能返回目标、耗时、HTTP 状态和出口信息。

用户最新要求先不推进 `warp-ops-enhancement`，因此下一批从 xray 节点生命周期、订阅源运维和验证矩阵中继续推进。WARP 保留为后续能力扩展，不作为当前 Ready/Next 主线。`xray-node-lifecycle-mvp` 和 `subscription-source-ops-mvp` 已完成，下一项推荐推进 `xray-config-dry-run-and-remove`。

**工作区注意事项**：

- 当前本地存在一组已隔离的 `update-failure-hardening` WIP：`stash@{0}: wip: paused update failure hardening`。按用户要求先不继续，不要默认恢复、删除或混入后续任务。
- 当前本地存在一组已隔离的 `fetcher-validator-quality` WIP：`stash@{1}: wip: paused fetcher circuit work`。不要默认恢复、删除或混入后续任务。
- 当前 Trellis 里 `gateway-route-debugging` 和 `fetcher-validator-quality` 已从 `in_progress` 改为 `paused`，当前会话任务指针已清空。
- `warp-ops-enhancement` 曾创建 planning 任务目录；按用户最新要求先不继续，任务状态保留为 `paused`，不作为 current task。
- `.codex/config.toml` 属于非本任务改动，不纳入任何 roadmap 提交或后续功能提交。
- 按用户要求，不直接 SSH 到 dev 地址；dev 验证默认走 HTTP、MCP、GitHub Actions、容器已有自更新入口和公开状态接口。

## Now

当前无 Now 任务；完成 `subscription-source-ops-mvp` 后，下一步从 Ready 选择 `xray-config-dry-run-and-remove`。

## Paused Closeout

### P0 — `update-failure-hardening`

**目标**：在不影响正常发布节奏的前提下，补齐自更新失败路径的故障注入验证。

**当前状态**：已开始过一个 WIP，但用户要求先不继续；当前草稿隔离在 `stash@{0}: wip: paused update failure hardening`，Trellis 任务指针已清空。后续需要安全窗口或更明确的 no-SSH 验证入口后再恢复。

**暂缓 TODO**：

- [ ] 错误镜像 tag / digest 时，`update_service` 返回结构化错误，旧容器继续运行。
- [ ] 错误 Watchtower token 时，`update_service` 返回结构化错误，旧容器继续运行。
- [ ] Watchtower HTTP endpoint 不可达时，`update_service` 返回结构化错误，旧容器继续运行。
- [ ] 形成一份可重复执行的 dev-only 验证步骤，避免误操作生产配置。
- [ ] 必要时增加自动化集成测试或最小脚本化检查。
- [ ] 验证方式仍遵循 no-SSH 规则，只使用 GitHub Actions、MCP、HTTP 状态接口和安全的 dev-only 配置入口。

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

### P1 — `xray-node-lifecycle-mvp`

**目标**：把 xray 节点从“只知道活跃数量”推进到可解释的生命周期状态，便于判断订阅节点是否成功进入 xray 出站池。

**当前状态**：已完成 MVP。`proxy-core` 现在提供共享 `XrayStatusRegistry` / `XrayStatusSnapshot`，`OutboundSync` 会记录 `pending`、`activating`、`active`、`failed`、`removed` 状态；xray inbound/outbound 或 ProxyStore 任一步失败时不再把节点伪装为 active，而是释放端口并把失败原因暴露给 API/MCP。

**主要完成项**：

- [x] 定义 xray 节点生命周期：`pending`、`activating`、`active`、`failed`、`removed`。
- [x] 记录节点 tag、协议、远端 host/port、本地 SOCKS5 port、状态、失败原因和更新时间。
- [x] `/api/xray/status` 返回 active/failed/removed 计数和最近节点状态。
- [x] MCP 新增 `xray_status`，`service_status` 也返回 active/failed/removed 摘要。
- [x] `/api/status` 和 Prometheus metrics 增加 xray failed 节点信息。
- [x] Dashboard 首页展示 xray 活跃/失败计数。
- [x] 同步 README、integration smoke 断言和 Trellis spec。
- [x] `cargo fmt --all --check` 通过。
- [x] `cargo test --workspace --all-targets` 通过。
- [x] `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- [x] `npm run build` 通过。

### P1 — `subscription-source-ops-mvp`

**目标**：补齐订阅源运维 MVP，让订阅源抓取、解析、去重和失败原因可被 API/MCP 查询，并支持安全的手动 preview/apply。

**当前状态**：已完成 MVP。`proxy-sub` 新增 `SubscriptionOpsHandle` 和结构化报告模型，后台刷新、REST API 与 MCP 共享同一状态；手动刷新默认 preview，不写入 `ProxyStore` 或 `PendingStore`，只有显式 `apply=true` 才会写入。

**主要完成项**：

- [x] 定义订阅源描述、刷新模式、刷新结果、错误列表和状态快照。
- [x] `/api/subscriptions/sources` 返回订阅源状态、最近报告和空配置状态。
- [x] `/api/subscriptions/sources/{id}/refresh` 支持单源手动 preview/apply，未知 source 返回 404。
- [x] MCP 新增 `subscription_sources` 和 `refresh_subscription_source`。
- [x] 刷新报告返回 discovered/unique/duplicate URL、parsed/direct/encrypted/unknown/duplicate node、stored count、protocol counts、elapsed 和 per-source errors。
- [x] API/MCP 响应不暴露原始订阅内容或完整节点凭据。
- [x] 后台订阅刷新继续运行，并复用同一个 ops 状态。
- [x] 同步 README、integration smoke 断言和 Trellis spec。
- [x] `cargo fmt --all --check` 通过。
- [x] `cargo test -p proxy-sub` 通过。
- [x] `cargo check -p proxy-api -p proxy-mcp -p proxy-server` 通过。
- [x] `cargo test --workspace --all-targets` 通过。
- [x] `cargo clippy --workspace --all-targets -- -D warnings` 通过。

### P1 — `validator-observability-v2`

**目标**：进一步提升 `check_proxy` 和批量验证结果的解释能力，说明代理“能连哪里、慢在哪里、出口是谁”。

**当前状态**：已完成 single-target diagnostics MVP。`ProxyCheckResult` 现在返回目标 URL/host、HTTP 状态、request/body/total 耗时，以及从 Cloudflare trace 或 httpbin JSON 中解析出的出口 IP/国家。MCP `check_proxy` 继续直接序列化核心 `Validator::check_one()` 结果。

**主要完成项**：

- [x] `ProxyCheckResult` 返回 `target_url` 和 `target_host`。
- [x] 验证结果返回 `timings.request_ms`、`timings.body_read_ms`、`timings.total_ms`。
- [x] 收到 HTTP 响应时返回 `http_status`，bad status 也携带诊断字段。
- [x] 从 Cloudflare trace 的 `ip=`/`loc=` 和 httpbin JSON 的 `origin` 解析出口信息。
- [x] MCP `check_proxy` 继续直接序列化核心 `Validator::check_one()` 结果，不在 MCP 层重复解析。
- [x] `validate_one()` 兼容批量验证，仍只在 alive 时返回 `Some(Proxy)`。
- [x] `cargo test --workspace --all-targets` 通过。
- [x] `cargo clippy --workspace --all-targets -- -D warnings` 通过。

**后续可选增强**：

- [ ] 多目标验证矩阵：默认目标、国内目标、国外目标、Cloudflare trace。
- [ ] 如需要精确 TCP/TLS 阶段耗时，单独评估是否引入更底层的连接探针。

### P1 — `fetcher-source-circuit-breaker-mvp`

**目标**：把抓取源从“只记录失败”推进到“连续失败后自动降噪，恢复时可探测”，降低坏源对代理池刷新质量的影响。

**当前状态**：已完成 MVP。`FetcherRunReport` 现在携带源级 circuit state、连续失败次数、最近错误、下次探测时间和 run action；自动刷新会跳过冷却期内 open source，冷却后进入 half-open probe；手动单源刷新可以 probe open source。API/MCP/Web 共享同一核心结构。

**主要完成项**：

- [x] 为每个 fetcher 维护连续失败次数、熔断状态、下次探测时间和最近错误。
- [x] 连续失败超过阈值后暂停该源，冷却期内跳过自动刷新。
- [x] 冷却期结束进入 half-open 探测，成功后关闭熔断，失败后延长冷却。
- [x] `fetcher_status` 和 `/api/fetchers` 展示熔断状态、失败原因和下次探测时间。
- [x] `refresh_fetcher` 对暂停源支持显式手动探测，并返回结构化结果。
- [x] Web Fetchers 页面展示真实 circuit state、失败次数、最近错误、下次探测和手动探测入口。
- [x] 未恢复或混入隔离的 `stash@{1}`。
- [x] `cargo test --workspace --all-targets` 通过。
- [x] `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- [x] `npm run build` 通过。

### P2 — `web-dashboard-real-ops-mvp`

**目标**：把 Web Dashboard 从演示/占位面板推进为真实可用的运维入口。

**当前状态**：已完成 MVP 切片。Dashboard、代理列表、路由诊断、抓取源、MCP Debug、日志入口和设置页现在使用真实 API 数据，或在后端能力不存在时显示明确不可用状态；不再用模拟日志或假默认配置冒充真实状态。

**主要完成项**：

- [x] 首页总览接入真实 `/api/status` 和 `/api/readyz`，展示版本、git hash、运行时间、Redis、WARP、xray 和代理池摘要。
- [x] Proxies 页面接入 `/api/proxies/scores`，展示评分、保留决策和评分组成。
- [x] Routes 页面移除不存在的规则编辑器，改为 `/api/routes/test` dry-run 诊断。
- [x] 新增 Fetchers 页面，对接 `/api/fetchers` 和 `/api/fetchers/{id}/refresh`。
- [x] MCP Debug 工具列表同步后端工具；REST 等效工具走真实 API，MCP-only 工具显示 transport-required。
- [x] Logs 页面移除模拟日志；Settings 页面移除不存在的 `/api/settings` 假默认配置。
- [x] WARP 页面改用 typed API helper，并禁用尚无 Web API 的优选动作。
- [x] `npm run build` 通过。

### P1 — `score-retention-policy`

**目标**：让代理评分、降权和清理策略更稳定、可解释。

**当前状态**：已完成首个可验收切片：现有 score 公式可解释，API/MCP 可查看 score explanation，MCP 低分清理默认 dry-run 且必须显式 `apply: true` 才会删除。

**主要完成项**：

- [x] 明确当前 score 计算公式并写入 `docs/score-retention.md`。
- [x] `proxy-core` 新增 score explanation 和 retention decision 模型。
- [x] REST 新增 `/api/proxies/scores`。
- [x] MCP 新增 `explain_proxy_scores`。
- [x] MCP 新增 `cleanup_low_score_proxies`，默认 dry-run，显式 `apply: true` 才清理。
- [x] 更新 README、集成测试期望和 `.trellis/spec/proxy-core/backend/quality-guidelines.md`。
- [x] `cargo test --workspace --all-targets` 通过。
- [x] `cargo clippy --workspace --all-targets -- -D warnings` 通过。

### P0 — `no-ssh-dev-validation`

**目标**：形成不依赖直接 SSH 的 dev 验证闭环，避免部署和故障验证依赖人工登录服务器。

**当前状态**：已完成文档和 helper hardening；后续 dev 验证默认走 GitHub Actions、MCP、HTTP 状态接口和集成测试，不走直接 SSH。

**主要完成项**：

- [x] 新增 `docs/dev-validation.md`，说明允许/禁止的验证入口和 post-push 检查清单。
- [x] `CLAUDE.md` 的部署验证流程明确禁止直接 SSH 到 dev 地址，并指向 `docs/dev-validation.md`。
- [x] `tests/integration/helpers/docker_control.py` 不再把 SSH 或 host Docker API 作为默认路径。
- [x] WARP fault-injection helper 不再静默通过，而是抛出 `FaultInjectionUnavailable`。
- [x] 新增 `tests/integration/test_l0_no_ssh_helpers.py` 覆盖 helper 拒绝 unsafe fault injection。
- [x] `python -m py_compile` 和 no-SSH helper 测试通过。

### P0 — `todo-queue-and-task-state-cleanup`

**目标**：让 Roadmap 和 Trellis 任务状态重新成为可信的执行入口，后续每次只推进一个明确任务。

**当前状态**：已完成本地清理并提交第一版新 Roadmap；旧任务未归档、未恢复 stash，只移除“正在做”的信号。

**主要完成项**：

- [x] Roadmap 新增 `Paused Closeout` 状态。
- [x] `gateway-route-debugging` 从 Now 移到 `Paused Closeout`，发布后文档/归档收尾按用户要求暂缓。
- [x] `fetcher-validator-quality` 保持暂缓，WIP 继续隔离在 `stash@{1}`。
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

### P2 — `xray-config-dry-run-and-remove`

**目标**：让 xray 运维动作可预检、可回退，减少错误配置直接写入运行态的风险。

**候选功能**：

- [ ] xray 配置变更 dry-run 校验。
- [ ] 手动移除单个 xray 节点。
- [ ] 记录移除原因和操作结果。
- [ ] MCP/API 返回结构化错误，便于 Web 和自动化工具展示。

## Next

### P2 — `validator-observability-multitarget`

**目标**：在 single-target diagnostics 稳定后，增加多目标验证矩阵，判断代理是否只对某些站点可用。

**候选功能**：

- [ ] 默认目标、Cloudflare trace、httpbin 和可选国内/国外目标的验证矩阵。
- [ ] 每个目标返回 HTTP 状态、耗时和出口信息。
- [ ] MCP `check_proxy` 保持单目标兼容，新增显式矩阵模式。

### P2 — `dashboard-ops-polish-v2`

**目标**：把新增的 xray、订阅源和验证矩阵能力接入 Web Dashboard，同时继续坚持只展示真实可用动作。

**候选功能**：

- [ ] xray 节点生命周期摘要和失败原因展示。
- [ ] 订阅源状态、刷新结果和解析预览展示。
- [ ] validator 多目标结果展示。
- [ ] 移除或禁用所有没有后端支持的操作按钮。

## Later

### P3 — `warp-ops-enhancement`

**目标**：增强 WARP endpoint 优选、健康检查和手动运维能力。当前按用户要求先不推进，保留 planning 草稿供后续恢复。

**候选功能**：

- [ ] 完善 WARP instance 状态模型：endpoint、latency、loss、healthy、assigned_at、fail_count。
- [ ] 查询最近 WARP optimizer 扫描结果。
- [ ] 支持手动触发 WARP endpoint 扫描。
- [ ] 支持 endpoint pinning，允许临时禁用 optimizer 覆盖。
- [ ] WARP 健康检查结果进入 Prometheus metrics。
- [ ] 增加 WARP 链式代理端到端测试。

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

- [ ] 源级熔断已拆为 `fetcher-source-circuit-breaker-mvp`。
- [ ] 验证结果可观测性已拆为 `validator-observability-v2`。

### P3 — `xray-subscription-ops`

**目标**：原 umbrella 任务，已拆分为 `xray-node-lifecycle-mvp`、`subscription-source-ops-mvp` 和 `xray-config-dry-run-and-remove`。后续如需更大的 xray/订阅源管理面板，再恢复该 umbrella。

**候选功能**：

- [ ] 增加 gRPC 重连状态指标。
- [ ] 订阅源新增、删除、启用、禁用。
- [ ] 更完整的 xray/订阅源 Web 管理体验。

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

1. `xray-node-lifecycle-mvp` — xray 节点生命周期和失败原因。
2. `subscription-source-ops-mvp` — 订阅源状态、手动刷新和解析预览。
3. `xray-config-dry-run-and-remove` — xray 配置 dry-run 和单节点移除。
4. `validator-observability-multitarget` — 多目标验证矩阵和更细阶段耗时。
5. `dashboard-ops-polish-v2` — 接入新增真实运维能力，移除假动作。
6. `warp-ops-enhancement` — 用户重新确认后再恢复 WARP 运维增强。
7. `update-failure-hardening` — 用户确认后再恢复自更新失败路径结构化错误和 no-SSH 验证。
8. `gateway-route-debugging` — 用户确认后再做任务归档、最终文档收尾或可选 debug header。

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
