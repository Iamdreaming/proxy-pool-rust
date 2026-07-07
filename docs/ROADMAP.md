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

当前已按用户要求暂不推进 `update-failure-hardening` 和 `xray-config-dry-run-and-remove`，并已完成 `web-dashboard-real-ops-mvp`、`fetcher-source-circuit-breaker-mvp`、`validator-observability-v2` 与 `validator-observability-multitarget`：Web Dashboard 现在优先展示真实运维数据或明确的不可用状态，抓取源具备源级熔断和手动探测能力，`check_proxy` 能返回目标、耗时、HTTP 状态和出口信息，`check_proxy_matrix` / `/api/proxy/check-matrix` 也能按多个目标返回验证矩阵。

用户最新要求先不做 `mcp-api-contract-smoke-v2`，因此该契约 smoke 草稿已暂停并隔离，不作为当前 Ready/Next 主线。`dashboard-ops-polish-v2` 也继续保持暂停。`proxy-quality-history-lite` 已完成；用户随后明确“先不做” `proxy-quality-recommendations-dry-run`，因此该 dry-run 建议任务只保留暂停草稿，不进入当前主线。

用户随后要求“先不做这个，规划新的 todo list”，因此 `revalidation-scheduler-priority-v1` 从当前主线移出并隔离。之后 `quality-dashboard-readonly-v1` 也按用户要求先不继续，当前 WIP 已隔离在 stash `wip: paused quality dashboard readonly`，不作为新的主线任务。新的 TODO 队列优先补齐 no-SSH、只读、低风险的发布验证和配置防漂移能力：先把已有 GitHub Actions、公开 HTTP 状态接口和 MCP 只读状态组合成可重复执行的本地 smoke runner，再做配置/文档漂移检查、Prometheus 低基数审计和最小只读契约 smoke。

**工作区注意事项**：

- 当前本地存在一组已隔离的 `dashboard-ops-polish-v2` WIP：`wip: paused dashboard ops polish v2`。按用户最新要求先不继续，不要默认恢复、删除或混入后续任务。
- 当前本地存在一组已隔离的 `mcp-api-contract-smoke-v2` WIP：`wip: paused mcp api contract smoke v2`。按用户最新要求先不继续，不要默认恢复、删除或混入后续任务。
- 当前本地存在一组已隔离的 `fetcher-source-quality-ranking` WIP：`wip: paused fetcher source quality ranking`。按用户最新要求先不继续，不要默认恢复、删除或混入后续任务。
- 当前本地存在一组已隔离的 `revalidation-scheduler-priority-v1` WIP：`wip: paused revalidation scheduler priority`。按用户最新要求先不继续，不要默认恢复、删除或混入后续任务。
- 当前本地存在一组已隔离的 `quality-dashboard-readonly-v1` WIP：`wip: paused quality dashboard readonly`。按用户最新要求先不继续，不要默认恢复、删除或混入后续任务。
- 当前 Trellis 中存在 `proxy-quality-recommendations-dry-run` 暂停草稿。按用户最新要求先不继续，不作为 Ready/Next 主线；后续只有用户重新确认后再恢复。
- 当前本地存在一组已隔离的 `update-failure-hardening` WIP：`wip: paused update failure hardening`。按用户要求先不继续，不要默认恢复、删除或混入后续任务。
- 当前本地存在一组已隔离的 `fetcher-validator-quality` WIP：`wip: paused fetcher circuit work`。不要默认恢复、删除或混入后续任务。
- 当前 Trellis 里 `gateway-route-debugging` 和 `fetcher-validator-quality` 已从 `in_progress` 改为 `paused`，当前会话任务指针已清空。
- `warp-ops-enhancement` 曾创建 planning 任务目录；按用户最新要求先不继续，任务状态保留为 `paused`，不作为 current task。
- `xray-config-dry-run-and-remove` 曾创建 planning 任务目录；按用户最新要求先不继续，任务状态保留为 `paused`，不作为 current task。
- `.codex/config.toml` 属于非本任务改动，不纳入任何 roadmap 提交或后续功能提交。
- 按用户要求，不直接 SSH 到 dev 地址；dev 验证默认走 HTTP、MCP、GitHub Actions、容器已有自更新入口和公开状态接口。

## Now

当前无 Now 任务；下一步建议推进 `metrics-low-cardinality-audit-v1`。`readonly-dev-smoke-runner-v1` 和 `config-runbook-drift-check-v1` 已完成，dev 发布验证默认保持 no-SSH、只读、本地可重复，并通过文档/配置漂移检查约束 compose env、release 字段和 Watchtower token 说明。`quality-dashboard-readonly-v1`、`revalidation-scheduler-priority-v1`、`update-failure-hardening`、`fetcher-source-quality-ranking`、`proxy-quality-recommendations-dry-run`、`mcp-api-contract-smoke-v2` 与 `dashboard-ops-polish-v2` 均按用户最新要求或安全门槛暂停，不作为当前主线。

## Paused Closeout

### P2 — `quality-dashboard-readonly-v1`

**目标**：在已有真实 Dashboard 基础上展示代理质量趋势、低质候选数量和近期失败原因，但不恢复已暂停的操作按钮草稿。

**当前状态**：已按用户最新要求“先不做这个”暂停。此前已创建过 Trellis 任务草稿并有少量前端类型/页面 WIP，当前已隔离在 stash `wip: paused quality dashboard readonly`；Trellis current 指针已清空。后续只有用户重新确认后再恢复，不纳入当前 Ready/Next 主线。

**暂缓 TODO**：

- [ ] 首页或 Proxies 页面展示质量趋势摘要、低质候选数量和近期失败原因。
- [ ] 如 `pool-quality-metrics-v1` 已完成，首页展示只读质量指标；否则显示明确的后端字段不可用状态。
- [ ] Proxies 页面展示 `/api/proxies/scores` 中已有的 per-proxy trend 字段。
- [ ] 所有新增 UI 都只消费真实后端字段；字段不可用时显示明确不可用状态。
- [ ] 不新增 apply 操作，不触发 `update_service`，不依赖直接 SSH。

### P1 — `revalidation-scheduler-priority-v1`

**目标**：让已有质量历史影响复验顺序，优先复查长期未检查、近期退化、失败压力高或来源风险高的代理，同时避免单一来源长期占满复验预算。

**当前状态**：已按用户最新要求“先不做这个”暂停。此前已有一组可继续的本地 WIP，已隔离在 stash `wip: paused revalidation scheduler priority`；Trellis current 指针已清空。后续只有用户重新确认后再恢复，不纳入当前 Ready/Next 主线。

**暂缓 TODO**：

- [ ] 定义复验候选优先级：last_checked、quality trend、fail_count、success_count、score、source quality 和 protocol 公平性。
- [ ] 调度器在不改变外部接口的前提下使用优先级排序，保留合理随机性或分桶公平性，避免饥饿。
- [ ] 对持续失败代理提高复验优先级但不直接清理；清理/降权策略仍留给后续单独任务。
- [ ] 暴露最小可观测字段或日志，说明本轮复验选择了哪些类别的代理。
- [ ] 覆盖排序规则、边界值和 scheduler revalidation 行为测试。

### P0 — `update-failure-hardening`

**目标**：在不影响正常发布节奏的前提下，补齐自更新失败路径的故障注入验证。

**当前状态**：已开始过一个 WIP，但用户要求先不继续；当前草稿隔离在本地 stash `wip: paused update failure hardening`，Trellis 任务指针已清空。后续需要安全窗口或更明确的 no-SSH 验证入口后再恢复。

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

### P2 — `dashboard-ops-polish-v2`

**目标**：把新增的 xray、订阅源、抓取源和验证能力接入 Web Dashboard，同时继续坚持只展示真实可用动作。

**当前状态**：用户最新要求“先不做这个”，因此暂停。已产生过一份前端草稿，但已经隔离在本地 stash `wip: paused dashboard ops polish v2`，不纳入当前 TODO 主线、不提交、不推送。

**暂缓 TODO**：

- [ ] xray 节点生命周期摘要和失败原因展示。
- [ ] 订阅源状态、刷新结果和解析预览展示。
- [ ] fetcher circuit state、手动 probe 结果和错误原因展示打磨。
- [ ] validator 多目标结果展示。
- [ ] 移除或禁用所有没有后端支持的操作按钮。

### P2 — `mcp-api-contract-smoke-v2`

**目标**：为最近新增的 REST/MCP 运维入口补齐契约级 smoke，减少 API 与 MCP 字段漂移。

**当前状态**：已创建过 Trellis 草稿并有少量文档/测试 WIP，但用户最新要求“先不做这个”。当前草稿已隔离在本地 stash `wip: paused mcp api contract smoke v2`，Trellis current 指针已清空，不纳入当前 TODO 主线、不提交、不推送。

**暂缓 TODO**：

- [ ] 统一列出 REST endpoint 与 MCP tool 的等价关系。
- [ ] 对 status、fetchers、subscriptions、xray status、route test、score explanation、proxy check matrix 增加轻量 smoke。
- [ ] 集成测试只依赖本地进程或公开 HTTP/MCP 入口，不依赖直接 SSH。
- [ ] README 中把运维入口按“可查询 / dry-run / apply”分类。

### P1 — `fetcher-source-quality-ranking`

**目标**：把代理质量回传到来源维度，帮助识别高产低质、长期失败、近期恢复或需要降噪的 fetcher/source。

**当前状态**：已按用户最新要求暂停。此前有一组可继续的本地 WIP，已隔离在 stash `wip: paused fetcher source quality ranking`；Trellis current 指针已清空。后续只有用户重新确认后再恢复，不纳入当前 Ready/Next 主线。

**暂缓 TODO**：

- [ ] 明确代理与来源之间的最小可追踪字段；旧数据缺少来源时保持兼容，不影响已有代理池读写。
- [ ] 为 fetcher/source 汇总 scored/alive counts、score buckets、recent success/failure、latency、circuit state 和 risk labels 等轻量指标。
- [ ] `/api/fetchers` 和 MCP `fetcher_status` 返回来源质量字段，adapter 层只序列化核心结构，不重复计算。
- [ ] 标记高风险来源：长期失败、解析成功但验证低、近期明显退化、熔断后仍反复失败。
- [ ] 不自动禁用来源、不删除代理；只给 operator、调度器和后续 Dashboard 提供依据。
- [ ] 覆盖 core 汇总逻辑、API/MCP shape 和旧数据兼容测试。

## Done

### P0 — `config-runbook-drift-check-v1`

**目标**：防止 README、`docs/dev-validation.md`、dev compose/env 示例和状态接口字段继续漂移，尤其是自更新相关环境变量与 no-SSH 验证边界。

**当前状态**：已完成。`docs/dev-validation.md` 已对齐实际 release 字段，使用 `release.configured_image`、`release.image_repo` 和 `release.image_tag`，不再记录过时的 `release.update_image`。runbook 也补齐 `PROXY_POOL_UPDATE_DOCKER_SOCKET`、Watchtower token 配对关系，以及 Watchtower 镜像可能缺少 `printenv` 的预期限制。

**主要完成项**：

- [x] 梳理并记录 dev compose 所需 `PROXY_POOL_UPDATE_*` 与 Watchtower token 对应关系。
- [x] 检查并修正 dev-validation / compose / status 字段描述中的 `release.update_image` 漂移。
- [x] 明确记录 Watchtower 镜像内缺少常规 shell 工具时属于预期限制，不把 `docker compose exec watchtower-proxy-pool printenv` 作为推荐验证方式。
- [x] 新增 `tests/integration/test_l0_config_runbook_drift.py`，本地只读检查 compose env、Watchtower token、release 字段和 no-SSH/no-routine-update 边界。
- [x] 新增 `.trellis/spec/integration/testing/config-runbook-drift-check.md`，沉淀后续修改部署验证文档时的可执行契约。
- [x] `python -m pytest tests\integration\test_l0_config_runbook_drift.py tests\integration\test_l0_no_ssh_helpers.py tests\integration\test_l0_readonly_dev_smoke.py -q` 通过。

### P0 — `readonly-dev-smoke-runner-v1`

**目标**：把 no-SSH 的 post-push dev 验证组合成本地可重复执行的一条命令，减少每次发布后人工拼接 GitHub Actions、HTTP 和 MCP 只读状态检查的成本。

**当前状态**：已完成本地 runner。`tests/integration/readonly_dev_smoke.py` 现在可以从仓库根目录运行，默认检查最新 `docker-build.yml`、公开 HTTP `/api/status` / `/api/readyz`、MCP `service_status` / `update_status`，并支持 `--skip-ci`、`--wait-ci` 和 `--json`。runner 不直接 SSH、不访问宿主 Docker、不调用 `update_service`，也不触发任何刷新、清理、删除或 apply 动作。

**主要完成项**：

- [x] 新增 `tests/integration/readonly_dev_smoke.py`，复用现有 `PROXY_POOL_*` 环境变量和 MCP HTTP helper。
- [x] 新增本地 L0 测试覆盖 hash 比对、状态 payload 判断、只读 MCP tool 集合和结果聚合。
- [x] `docs/dev-validation.md` 增加 runner 快捷命令，同时保留 manual triage 步骤。
- [x] README 的 Dev 验证段落增加 runner 示例。
- [x] `python -m py_compile tests\integration\readonly_dev_smoke.py tests\integration\test_l0_readonly_dev_smoke.py tests\integration\conftest.py tests\integration\helpers\mcp_client.py tests\integration\config.py` 通过。
- [x] `python -m pytest tests\integration\test_l0_readonly_dev_smoke.py -q` 通过。
- [x] live read-only smoke 已运行：GitHub Actions 检查成功，HTTP/MCP 检查正确发现当前 dev runtime 仍缺少 release fields 和 MCP `update_status`。

### P0 — `release-validation-no-ssh-runbook-v2`

**目标**：把推送后如何判断 dev 是否运行目标镜像、目标 git hash 和最近更新状态整理成可重复执行的 no-SSH 验证清单。

**当前状态**：已完成文档型交付。`docs/dev-validation.md` 现在把默认 post-push 验证定义为只读流程：GitHub Actions、公开 HTTP status/readyz、MCP `service_status` 和 MCP `update_status`；`update_service` 被明确归类为 operator 显式选择的 mutating update action，不再是默认状态检查。README 和 CLAUDE 指令也已同步该边界。

**主要完成项**：

- [x] 默认 post-push checklist 不再触发 `update_service`。
- [x] 文档列出允许入口：GitHub Actions、公开 HTTP 状态接口、MCP `service_status` / `update_status` 和 feature smoke 工具。
- [x] 文档明确禁止直接 SSH 到 dev 地址、host Docker CLI/API 访问，以及把 `update_service` 当作 routine status check。
- [x] 文档记录 dev compose 更新可观测性所需环境变量：update enabled、container/image、Watchtower URL 和 token 对应关系。
- [x] 文档包含 CI、镜像、runtime git hash、release metadata 和 update status 的失败分支判断。
- [x] README 和 CLAUDE.md 已同步 no-SSH 默认只读验证说明。

### P0 — `release-status-contract-smoke-v1`

**目标**：为 no-SSH 发布验证依赖的最小状态契约补轻量 smoke，避免 `/api/status`、MCP `service_status` 和 MCP `update_status` 的关键字段漂移。

**当前状态**：已完成最小契约 smoke。REST `/api/status` integration shape 断言覆盖 `release` metadata；MCP `service_status` 覆盖同一 release metadata；MCP 工具列表包含 `update_status`，并新增只读 `update_status` shape smoke。测试没有调用 `update_service`，也不依赖 SSH、Docker socket、Watchtower 或 live dev mutation。

**主要完成项**：

- [x] `/api/status` shape 断言覆盖 top-level `version` / `git_hash` 与 `release.app_version` / `release.git_hash` 的对应关系。
- [x] `/api/status` shape 断言覆盖 `release.update_enabled`、`update_container`、`configured_image`、`image_repo`、`image_tag` 和 `watchtower_url`。
- [x] MCP `service_status` shape 断言覆盖同一组 release metadata。
- [x] MCP `update_status` 只读 smoke 覆盖 `never_triggered`、`disabled`、`already_current`、`updated` 和 `failed` 状态集合。
- [x] 已有 `proxy-mcp` 单测继续覆盖 disabled/failed/already_current/updated 等 recorded update status shape。
- [x] `python -m py_compile tests\integration\test_l2_api.py tests\integration\test_l4_mcp.py` 通过。
- [x] `cargo test -p proxy-api` 通过。
- [x] `cargo test -p proxy-mcp` 通过。

### P2 — `pool-quality-metrics-v1`

**目标**：把代理池质量趋势和保留风险沉淀为只读 metrics/status 字段，便于 no-SSH 环境下判断代理池是否正在变好。

**当前状态**：已完成共享只读质量摘要。`proxy-core::status::ServiceStatus` 现在包含 `quality` 对象；REST `/api/status` 和 MCP `service_status` 复用同一结构；`/api/metrics` 输出低基数质量指标。失败原因会归一化为 bounded reason label，不把代理地址、完整 URL、订阅内容或原始错误字符串作为 Prometheus label。

**主要完成项**：

- [x] `ServiceStatus.quality` 返回 total、score buckets、recent samples、recent success rate、recent failures、stale proxy count、retention-risk counts 和 normalized top failure reasons。
- [x] Redis 质量扫描失败时，状态仍返回默认 quality，且 `redis.status=error` 暴露失败。
- [x] `/api/metrics` 增加 `proxy_quality_score_bucket`、`proxy_quality_recent_samples_total`、`proxy_quality_recent_success_rate`、`proxy_quality_recent_failures_total`、`proxy_quality_stale_proxies_total`、`proxy_quality_retention_candidates` 和 `proxy_quality_failure_reasons_total`。
- [x] REST `/api/status` 和 MCP `service_status` integration smoke 已覆盖 `quality` shape。
- [x] README、`docs/score-retention.md` 和 `.trellis/spec/proxy-core/backend/quality-guidelines.md` 已同步质量摘要契约。
- [x] `cargo fmt --all --check` 通过。
- [x] `cargo test -p proxy-core` 通过。
- [x] `cargo test -p proxy-api` 通过。
- [x] `cargo test -p proxy-mcp` 通过。
- [x] `cargo test --workspace --all-targets` 通过。
- [x] `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- [x] `python -m py_compile tests\integration\test_l2_api.py tests\integration\test_l4_mcp.py` 通过。

### P1 — `proxy-quality-history-lite`

**目标**：在现有评分解释基础上，记录轻量级质量趋势，帮助判断代理是短暂波动还是持续变差。

**当前状态**：已完成 MVP。`Proxy` 现在带有向后兼容的 bounded `quality_history`，旧 Redis JSON 缺少该字段时会默认反序列化为空历史；`ScoreExplanation` 新增 `trend` 对象，REST `/api/proxies/scores` 与 MCP `explain_proxy_scores` 继续直接序列化 `proxy-core` 的 `ScoredProxy`，不在 adapter 层重算趋势。

**主要完成项**：

- [x] 为代理保留最近 10 次验证摘要，包含成功/失败、检查时间、成功 latency 和失败原因。
- [x] `mark_success`、`mark_failed`、`mark_failed_with_circuit` 和成功 revalidation 写路径会更新质量历史。
- [x] `ScoreExplanation.trend` 返回 `recent_samples`、`recent_success_rate`、`recent_latency_p50`、`recent_failures` 和 `last_checked_at_unix_secs`。
- [x] `score()` 数值公式和 Redis sorted-set score 语义保持兼容。
- [x] REST/MCP 单元测试和 integration shape 断言已覆盖 trend 字段。
- [x] `docs/score-retention.md` 和 `.trellis/spec/proxy-core/backend/quality-guidelines.md` 已同步趋势契约。
- [x] `cargo fmt --all --check` 通过。
- [x] `cargo test -p proxy-core` 通过。
- [x] `cargo test -p proxy-api` 通过。
- [x] `cargo test -p proxy-mcp` 通过。
- [x] `cargo test --workspace --all-targets` 通过。
- [x] `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- [x] `python -m py_compile tests\integration\test_l2_api.py tests\integration\test_l4_mcp.py` 通过。

### P2 — `release-observability-no-ssh-v2`

**目标**：继续强化不直接 SSH 的发布验证闭环，让 dev 是否已运行目标镜像、最近更新状态和失败原因更容易通过公开入口判断。

**当前状态**：已完成 MVP。`/api/status` 和 MCP `service_status` 现在共享 `release` metadata，包含版本、git hash、配置镜像、镜像 repo/tag、更新容器、Watchtower URL 和更新开关；MCP 新增只读 `update_status`，可查询最近一次 `update_service` 结果，不触发 Docker/Watchtower 操作。

**主要完成项**：

- [x] `/api/status.release` 暴露 release metadata，且不依赖 Docker socket。
- [x] MCP `service_status.release` 复用同一共享状态模型。
- [x] MCP 新增 `update_status`，返回 `never_triggered`、`disabled`、`already_current`、`updated` 或 `failed`。
- [x] `update_service` 在 disabled、token/config error、Docker inspect/pull error、already current、Watchtower success 和 Watchtower failure 路径记录最近结果。
- [x] README、`docs/dev-validation.md` 和 Trellis MCP spec 已同步 no-SSH post-push 验证说明。
- [x] `cargo fmt --all --check` 通过。
- [x] `cargo test -p proxy-core` 通过。
- [x] `cargo test -p proxy-mcp` 通过。
- [x] `cargo test -p proxy-api` 通过。
- [x] `cargo check -p proxy-server` 通过。
- [x] `cargo test --workspace --all-targets` 通过。
- [x] `cargo clippy --workspace --all-targets -- -D warnings` 通过。

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

- [ ] 如需要精确 TCP/TLS 阶段耗时，单独评估是否引入更底层的连接探针。

### P2 — `validator-observability-multitarget`

**目标**：在 single-target diagnostics 稳定后，增加多目标验证矩阵，判断代理是否只对某些站点可用。

**当前状态**：已完成 MVP。`proxy-core` 现在提供 `ProxyCheckMatrixRequest` / `ProxyCheckMatrixResult` 和 `check_proxy_matrix()`，默认检查 Cloudflare trace 与 httpbin IP；REST 和 MCP 入口都直接序列化核心结果，不在 adapter 层重新拼字段。

**主要完成项**：

- [x] 默认目标矩阵：`https://www.cloudflare.com/cdn-cgi/trace` 和 `https://httpbin.org/ip`。
- [x] 每个目标复用 `Validator::check_one()`，返回 HTTP 状态、耗时、出口 IP/国家和稳定错误类型。
- [x] 新增 REST `POST /api/proxy/check-matrix`。
- [x] 新增 MCP `check_proxy_matrix`，并保持 `check_proxy` 单目标行为兼容。
- [x] 输入校验在网络调用前完成：空 host、0 端口、无效协议、无效 target URL、非法 timeout 返回结构化错误。
- [x] README、integration smoke 断言和 Trellis spec 已同步。
- [x] `cargo fmt --all --check` 通过。
- [x] `cargo test -p proxy-core` 通过。
- [x] `cargo test -p proxy-api` 通过。
- [x] `cargo test -p proxy-mcp` 通过。

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
- [x] 未恢复或混入隔离的 `wip: paused fetcher circuit work`。
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
- [x] `fetcher-validator-quality` 保持暂缓，WIP 继续隔离在本地 stash `wip: paused fetcher circuit work`。
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

### P2 — `metrics-low-cardinality-audit-v1`

**目标**：系统性审计 Prometheus 指标 label，确保最近新增质量、路由、fetcher、release 指标持续保持低基数。

**候选功能**：

- [ ] 列出当前 `/api/metrics` 中的业务指标和 label。
- [ ] 确认没有代理地址、完整 URL、订阅内容、原始错误字符串、容器动态 ID 等高基数字段进入 label。
- [ ] 对失败原因、协议、bucket、状态类 label 给出允许值或归一化规则。
- [ ] 必要时补测试或文档，防止后续新增指标破坏低基数约束。

### P0 — `release-status-public-smoke-v1`

**目标**：在不恢复完整 REST/MCP 契约 smoke 的前提下，为公开只读发布状态补一组更轻的 smoke，覆盖发布验证真正依赖的字段。

**候选功能**：

- [ ] `/api/status` smoke 覆盖 `git_hash`、`release`、`quality` 和依赖状态字段。
- [ ] `/api/readyz` smoke 覆盖 HTTP 200/503 与结构化依赖状态。
- [ ] MCP `service_status` / `update_status` smoke 覆盖只读字段形状，不调用 `update_service`。
- [ ] 作为 `readonly-dev-smoke-runner-v1` 的本地验证基础之一。

## Next

### P0 — `dev-update-config-doc-hardening-v1`

**目标**：把当前 dev compose 自更新配置沉淀为标准说明，降低后续排障时误判环境变量、token 或 Watchtower 行为的概率。

**候选功能**：

- [ ] 文档化 `proxy-pool` 与 `watchtower-proxy-pool` 的容器职责、环境变量和 labels。
- [ ] 明确 dev 默认配置已经支持 `PROXY_POOL_UPDATE_ENABLED=true`、Watchtower HTTP API 和 GHCR latest 镜像。
- [ ] 给出只读验证方式：从 `proxy-pool` 容器环境、公开 status/update_status 和 GitHub Actions 判断，而不是直接 SSH。
- [ ] 给出安全回滚思路：回退镜像 tag/digest 或暂停 update action，具体 apply 操作仍需人工显式执行。

### P0 — `api-readonly-contract-minimal-v1`

**目标**：把当前真正用于自动验证的只读 API/MCP 字段整理成最小契约，避免重新打开已暂停的完整 `mcp-api-contract-smoke-v2` 范围。

**候选功能**：

- [ ] 定义 `/api/status`、`/api/readyz`、`/api/metrics`、`/api/proxies/scores` 的最小只读字段集合。
- [ ] 定义 MCP `service_status`、`update_status` 和 `explain_proxy_scores` 的最小只读字段集合。
- [ ] 明确不覆盖 mutating tools、不覆盖全部运维入口、不触发 `update_service`。
- [ ] 为后续 runner 和 smoke 任务提供稳定字段清单。

## Later

### P1 — `proxy-quality-recommendations-dry-run`

**目标**：基于当前分数和轻量质量历史，给出可解释的清理/降权建议，但默认不修改代理池。当前按用户要求先不推进，保留 paused Trellis 草稿供后续恢复。

**候选功能**：

- [ ] MCP/API 提供 dry-run 建议入口，返回候选代理、原因、预期动作和风险说明。
- [ ] 建议规则同时考虑 score、最近成功率、延迟和连续失败。
- [ ] 默认不删除、不降权、不刷新；未来 apply 入口需单独任务确认。
- [ ] 保留可测试的规则函数，避免 API/MCP adapter 重复计算。

### P2 — `xray-config-dry-run-and-remove`

**目标**：让 xray 运维动作可预检、可回退，减少错误配置直接写入运行态的风险。当前按用户要求先不推进，保留 paused Trellis 草稿供后续恢复。

**候选功能**：

- [ ] xray 配置变更 dry-run 校验。
- [ ] 手动移除单个 xray 节点。
- [ ] 记录移除原因和操作结果。
- [ ] MCP/API 返回结构化错误，便于 Web 和自动化工具展示。

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
3. `validator-observability-multitarget` — 已完成，多目标验证矩阵和更细阶段耗时。
4. `release-observability-no-ssh-v2` — 已完成，发布状态、镜像元数据和最近更新结果的 no-SSH 可观测性。
5. `proxy-quality-history-lite` — 已完成，代理质量轻量趋势和只读解释字段。
6. `release-validation-no-ssh-runbook-v2` — 已完成，把 post-push dev 验证清单固定为 GitHub Actions、公开 HTTP 和 MCP 只读入口。
7. `release-status-contract-smoke-v1` — 已完成，为发布验证依赖的 status/update_status 字段补最小契约 smoke，不恢复完整 REST/MCP smoke。
8. `pool-quality-metrics-v1` — 已完成，将质量趋势和保留风险暴露为低基数只读指标。
9. `readonly-dev-smoke-runner-v1` — 已完成，把 no-SSH 只读验证组合成一条本地可重复命令。
10. `config-runbook-drift-check-v1` — 已完成，防止 README、dev-validation、compose/env/status 字段继续漂移。
11. `metrics-low-cardinality-audit-v1` — 系统性审计 Prometheus label 是否持续保持低基数。
12. `release-status-public-smoke-v1` — 为公开只读发布状态补更轻的 smoke，不恢复完整 REST/MCP smoke。
13. `dev-update-config-doc-hardening-v1` — 沉淀当前 dev compose 自更新配置和只读排障说明。
14. `api-readonly-contract-minimal-v1` — 定义 runner/smoke 依赖的最小只读 API/MCP 字段集合。
15. `quality-dashboard-readonly-v1` — 用户重新确认后再恢复，只读展示质量趋势，不恢复暂停的操作按钮草稿。
16. `revalidation-scheduler-priority-v1` — 用户重新确认后再恢复，让质量历史影响复验优先级，但不直接清理代理。
17. `update-failure-hardening` — 用户确认安全验证入口后再恢复自更新失败路径结构化错误和 no-SSH 验证。
18. `fetcher-source-quality-ranking` — 用户重新确认后再恢复，来源维度质量排名和退化提示。
19. `proxy-quality-recommendations-dry-run` — 用户重新确认后再恢复，基于趋势输出清理/降权建议，默认 dry-run。
20. `mcp-api-contract-smoke-v2` — 用户重新确认后再恢复 REST/MCP 运维入口契约 smoke。
21. `dashboard-ops-polish-v2` — 用户重新确认后再恢复 Dashboard 运维整合草稿。
22. `xray-config-dry-run-and-remove` — 用户重新确认后再恢复 xray 配置 dry-run 和单节点移除。
23. `warp-ops-enhancement` — 用户重新确认后再恢复 WARP 运维增强。
24. `gateway-route-debugging` — 用户确认后再做任务归档、最终文档收尾或可选 debug header。

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
