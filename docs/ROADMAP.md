# proxy-pool-rust Roadmap

> 本文档是项目级功能路线图，负责记录长期方向、优先级和可拆分任务。具体实现细节、验收标准和执行步骤应落到 `.trellis/tasks/` 中的独立任务。

## 管理原则

1. **Roadmap 管方向**：只记录优先级、范围和任务拆分，不替代 PRD / design / implement 文档。
2. **Trellis 管执行**：进入开发前，为可独立验收的功能创建 `.trellis/tasks/<task>/prd.md`。
3. **一次只做一个 In Progress / Now（1-WIP）**：同时最多 1 个 `in_progress` 任务与 1 个 Now 条目，避免多条半开 WIP 与失真 Now。
4. **新想法进 Parking Lot**：未经确认的方向先写入 Parking Lot，不直接堆半截 stash，也不抢占 Now。
5. **暂停仪式**：更新 ROADMAP 对应行（Keep-Later）→ 可选 `git stash push -m "wip: paused <slug>"` → 清空 Trellis current task；**禁止**默认 `stash drop/apply`。
6. **完成仪式**：归档 Trellis 任务 → 更新 ROADMAP Now/Done → 需要时再从 Ready 经 `task.py start` 开工下一条。
7. **每个任务必须有验收标准**：没有验收标准的 TODO 先留在 Parking Lot。
8. **完成后更新本文档**：每完成、暂停或取消一个任务，都同步调整状态。

## 状态定义

| 状态 | 含义 |
|------|------|
| Now | 当前正在做，最多 1 个 |
| Ready | PRD 清楚、验收标准明确，可以排队开工 |
| Next | 优先级较高，但还需要细化 PRD |
| Keep-Later | 已暂停的 WIP/草稿；保留 stash 或 archive，待用户确认后再 Resume（原 Paused Closeout） |
| Later | 后续增强，不阻塞近期迭代 |
| Parking Lot | 想法池，暂不承诺实现 |
| Done | 已完成并验证（历史记录） |

## 优先级定义

| 优先级 | 含义 | 示例 |
|--------|------|------|
| P0 | 阻塞「持续可用」 | 可靠出口默认路径、网关失败反馈、脏窗口缩短、服务存活信号 |
| P1 | 出口质量与供给 | xray 准入/复验、WARP 健康、复验优先级、free pool 降权 |
| P2 | 观测与运维效率 | metrics 契约、route dry-run、最小只读 API/MCP 契约 |
| P3 | 能力扩展 / UI | Dashboard、WARP pinning、订阅自动发现、多区域调度 |

> **可用性收敛期**：P2/P3 不得默认抢占 Now；Keep-Later 平台项默认不 Resume。

## Availability-First

**单一目标**：客户端连 Gateway `:9080`，在需要的时段内尽量一直有可用出口。

**健康定义**（三元 AND，日常 Go/No-Go）：

1. Gateway 进程/端口可用（数据面在线）
2. `pool.tier` ≥ `minimal`（至少 1 个健康 WARP）；目标态 `stable`（xray active ≥ 3 且 WARP healthy ≥ 1）
3. 业务 smoke 通过（真实目标站，而非仅 Cloudflare trace）

不把「池子总数很大」「MCP 工具很多」「Dashboard 好看」算成功。

### 分层（L0–L4）

| 层 | 组件 | 策略 |
|----|------|------|
| L0 | Gateway + Redis + 复验调度 + 路由回退 | 必须稳；改动优先保证这里 |
| L1 | WARP 健康实例 + xray 激活节点 + 干净订阅 | **主供给**；海外默认走这里 |
| L2 | free pool 抓取/评分/熔断 | 可选兜底；**不进 premium**；冻结扩张 |
| L3 | status/readyz/metrics 关键集、route_test、业务 smoke | 服务排障，最小集 |
| L4 | Dashboard、完整 MCP 契约、订阅自动发现全家桶、WARP optimizer 花活 | **冻结** |

### 保 / 冻 / 后做

| 类别 | 内容 |
|------|------|
| **保** | Gateway HTTP CONNECT/SOCKS5、QualityTier 回退、WARP 健康、xray 准入复验、Redis 评分/circuit、`/api/status` `/readyz`、业务 e2e smoke、route_test |
| **冻** | Dashboard 打磨、mcp-api-contract-smoke-v2、质量 dashboard、fetcher 来源排名、订阅 GitHub/LLM 自动发现扩源、多租户/鉴权/告警集成 |
| **后做** | Ready 中 P0-B/C（网关失败反馈、脏窗口硬化）；P0-A 已完成见 Done |

**心智**：个人稳定代理出口网关（xray + WARP 主用），不是免费代理抓取平台。

## Current Planning Decision

**可用性优先收敛**（`availability-first-convergence`）为当前方向决策：重排优先级与队列，**本任务只改文档与任务地图，不改网关/调度业务语义**。

- **过程债结论仍有效**：`process-debt-convergence` 已归档；7 条 `wip: paused ...` stash 保持 Keep-Later（**禁止**默认 drop/apply/pop/clear）；明细见  
  `.trellis/tasks/archive/2026-07/07-18-process-debt-convergence/inventory.md`。
- **D2**：Now 仅允许 1 条；须经 Trellis `task.py start` 写入，禁止未 start 即写 Now。
- **可用性收敛期**：Ready 以 P0-B/C 为准（P0-A 已 Done）；P2 契约/UI 与 Keep-Later 平台项 **不抢 Now**、**默认不 Resume stash**。
- `reliable-exit-defaults-v1` 已完成并归档（见 Done），不再占用 Now。
- `metrics-low-cardinality-audit-v1` 已完成并归档（见 Done），不再占用 Now。
- 不直接 SSH 到 dev；默认验证仍走 GitHub Actions、公开 HTTP 状态与 MCP 只读入口（见 `docs/dev-validation.md`）。

## Now

### P0 — `gateway-failure-feedback-v1`

**目标**：网关上游失败后，后续选择在反馈窗口内避开同一坏出口（free_pool / xray；短 TTL Redis + 进程内）。

**当前状态**：`in_progress`；任务目录 `.trellis/tasks/07-19-gateway-failure-feedback-v1`。

**范围摘要**：`record_upstream_attempt` 写 Redis cooldown keys；`try_pool`/`try_xray` 过滤；不写 score/circuit；无 MCP/Dashboard。

## Keep-Later

> **可用性收敛期默认不 Resume。** 明细与恢复注意以  
> `.trellis/tasks/archive/2026-07/07-18-process-debt-convergence/inventory.md`  
> 为准。stash **message** 为稳定键；`stash@{n}` 会漂移。处置当前均为 Keep-Later。

| slug | P | stash message | intent (1 line) | disposition | restore |
|------|---|---------------|-----------------|-------------|---------|
| `quality-dashboard-readonly-v1` | P2 | `wip: paused quality dashboard readonly` | 只读展示质量趋势/低质候选，不恢复操作按钮 | Keep-Later | 新任务 + message 定位；可能与 main 漂移 |
| `revalidation-scheduler-priority-v1` | P1 | `wip: paused revalidation scheduler priority` | 用质量历史影响复验优先级，不直接清理 | Keep-Later | 触及 `scheduler.rs`；apply 前评估冲突 |
| `update-failure-hardening` | P0 | `wip: paused update failure hardening` | 自更新失败路径结构化错误与 no-SSH 验证 | Keep-Later | 需安全窗口；禁误触 live update |
| `dashboard-ops-polish-v2` | P2 | `wip: paused dashboard ops polish v2` | Dashboard 接入 xray/订阅/fetcher/validator 真实运维展示 | Keep-Later | 前端草稿；勿与 quality dashboard 混 apply |
| `mcp-api-contract-smoke-v2` | P2 | `wip: paused mcp api contract smoke v2` | 完整 REST/MCP 运维契约 smoke（大于最小只读契约） | Keep-Later | 勿与 `api-readonly-contract-minimal-v1` 混做 |
| `fetcher-source-quality-ranking` | P1 | `wip: paused fetcher source quality ranking` | 来源维度质量排名与风险标签（只读依据） | Keep-Later | 多文件大 diff；恢复成本高 |
| `fetcher-validator-quality`（残余 WIP） | P1 | `wip: paused fetcher circuit work` | 历史 fetcher 增强草稿；源级熔断等已拆分完成 | Keep-Later | apply 前对照现行 main；部分能力可能已落地 |

另：下列 Later 项仅有 **archive / 历史草稿**，无 active 任务目录，也不在上述 7 stash 内（或仅部分重叠）——见 inventory「Roadmap drafts」表：`proxy-quality-recommendations-dry-run`、`xray-config-dry-run-and-remove`、`warp-ops-enhancement`。

## Done

> 以下为**历史完成**记录，非当前主线。当前执行信号以 §Now / §Ready / §Keep-Later 为准。

### P0 — `reliable-exit-defaults-v1`

**目标**：overseas-stable example（`default`→premium）+ default 命中 GeoIP 国内 Direct；README/settings 对齐；单测锁定。

**当前状态**：已完成；任务目录归档至 `.trellis/tasks/archive/2026-07/07-19-reliable-exit-defaults-v1`（归档提交后路径生效）。

**主要完成项**：

- [x] `config/routes.example.yaml`：overseas-stable；`default`∈premium `overseas`；`*.cn`→direct；domestic-friendly 注释
- [x] default 非 direct-only + GeoIP：国内 Direct / 境外组 tier；direct-only default 不被改写
- [x] `settings.example.yaml` `routes_path` 指引；README 路由决策链与 QualityTier 一致
- [x] 单测锁定 example + GeoIP refine gate；spec（scenario-quality-tiers / quality-guidelines）同步

### P0 — `availability-first-convergence`

**目标**：把执行主线收敛为「持续正常使用代理」：重写优先级、Availability-First 分层、Ready P0 地图与冻结清单（文档 only）。

**当前状态**：已完成；任务目录归档至 `.trellis/tasks/archive/2026-07/07-19-availability-first-convergence`（归档提交后路径生效）。

**主要完成项**：

- [x] 优先级定义改为可用性优先（P0=持续可用）
- [x] Availability-First 节：目标句、健康定义、L0–L4、保/冻/后做
- [x] Ready：P0-A / P0-B / P0-C 地图（目标/非目标/验收草稿）
- [x] `api-readonly-contract-minimal-v1` 降为 Later(P2)；Keep-Later 默认不 Resume
- [x] metrics 审计收尾进 Done；Now 置空
- [x] 无 gateway/scheduler 业务代码变更

### P2 — `metrics-low-cardinality-audit-v1`

**目标**：系统性审计 `/api/metrics` 业务指标与 label，用测试与 spec 锁死低基数约束。

**当前状态**：已完成并归档到 `.trellis/tasks/archive/2026-07/07-19-metrics-low-cardinality-audit-v1`。

**主要完成项**：

- [x] 全量指标+label 清单写入 `quality-guidelines.md`（Prometheus Low-Cardinality Contract）
- [x] 白名单 / gateway 45 series / failure-reason 负向测试
- [x] 明确 fetcher/release 当前无 metrics；未来新增须遵守同一规则
- [x] 不新增业务指标，不抽共享 helper

### P1 — `process-debt-convergence`

**目标**：阶段 0 止血 + 阶段 1 过程债收敛（ROADMAP 校准、Keep-Later 台账、本地噪音清理），不恢复 paused 功能。

**当前状态**：已完成并归档到 `.trellis/tasks/archive/2026-07/07-18-process-debt-convergence`。

**主要完成项**：

- [x] Now 与 Trellis/归档一致；收尾后无业务 Now。
- [x] `inventory.md` 覆盖 7 条 `wip: paused ...` stash + 相关 Later/archive 草稿，处置均为 Keep-Later。
- [x] 过期 agent worktree 移除；`temp/` / `.tmp_verify/` / `protoc.exe` 清理；`cargo clean`（约 47G target）。
- [x] `proxy_ip_suggest.txt` 取消跟踪并 gitignore。
- [x] ROADMAP 管理原则固化 1-WIP、暂停/完成仪式；Paused 散文改为 Keep-Later 表；Ready/Next 不重排。
- [x] **未** stash drop/apply；**未**恢复业务功能代码。

### P1 — `gateway-route-debugging`

**目标**：让网关路由决策和 fallback 链路可解释、可观测、可测试。

**当前状态**：已完成并归档到 `.trellis/tasks/archive/2026-07/07-07-gateway-route-debugging`。核心实现已落地并推送到 `2842043 feat: add gateway route diagnostics`，收尾验证记录见任务目录的 `verification.md`。

**主要完成项**：

- [x] 为网关请求记录 route rule、GeoIP 结果、出口选择、fallback 候选和最终选择。
- [x] 新增 route dry-run 能力：输入 host/protocol，返回命中规则、GeoIP、出口和 fallback 顺序。
- [x] MCP 增加 `route_test` 工具。
- [x] 对 gateway route attempts 增加 Prometheus 指标。
- [x] 增加 gateway / API / MCP / core 相关自动化测试。
- [x] 本地 focused closeout 验证通过：`cargo test -p proxy-core route_debug`、`cargo test -p proxy-api route_test`。
- [x] 通过 no-SSH、no-mutation 的公开 HTTP 检查确认 dev 上 `/api/routes/test` 和 `/api/metrics` 可用。

**后续可选项**：

- [ ] 如未来需要浏览器/客户端内联诊断，再单独规划配置开关控制的 debug header。

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

### P0 — `release-status-public-smoke-v1`

**目标**：在不恢复完整 REST/MCP 契约 smoke 的前提下，为公开只读发布状态补一组更轻的 smoke，覆盖发布验证真正依赖的字段。

**当前状态**：已完成。新增共享 `helpers.release_status` 断言与轻量 public smoke；默认只校验公开状态结构，只有显式设置 `PROXY_POOL_GIT_HASH` 时才比对目标运行版本，避免本地 HEAD 超前 dev 时误判。测试不调用 `update_service`，也不触发刷新、删除、清理或 apply。

**主要完成项**：

- [x] 新增 `tests/integration/helpers/release_status.py`，集中维护 release/status/readyz/update_status 字段契约。
- [x] 新增 `tests/integration/test_l0_release_status_public_smoke.py`，本地纯测试覆盖 helper、hash mismatch、readyz、已知 update status 和只读 MCP tool 集合。
- [x] 新增 `tests/integration/test_release_status_public_smoke.py`，通过公开 HTTP/MCP 只读入口检查 `/api/status`、`/api/readyz`、MCP `service_status` 和 MCP `update_status`。
- [x] `tests/integration/test_l2_api.py` 与 `tests/integration/test_l4_mcp.py` 复用共享 release/status helper，减少字段契约重复。
- [x] 新增 `.trellis/spec/integration/testing/release-status-public-smoke.md`，沉淀轻量 smoke 的 no-SSH、no-mutation 和显式 hash 比对契约。
- [x] `python -m py_compile tests\integration\helpers\release_status.py tests\integration\test_l0_release_status_public_smoke.py tests\integration\test_release_status_public_smoke.py tests\integration\test_l2_api.py tests\integration\test_l4_mcp.py` 通过。
- [x] `python -m pytest tests\integration\test_l0_release_status_public_smoke.py tests\integration\test_release_status_public_smoke.py -q` 通过。
- [x] `python -m pytest tests\integration\test_l2_api.py::TestApiStatus::test_status_returns_version tests\integration\test_l4_mcp.py::TestMcpServiceStatus::test_service_status_structure tests\integration\test_l4_mcp.py::TestMcpServiceStatus::test_update_status_read_only_structure -q` 通过。

### P0 — `dev-update-config-doc-hardening-v1`

**目标**：把当前 dev compose 自更新配置沉淀为标准说明，降低后续排障时误判环境变量、token 或 Watchtower 行为的概率。

**当前状态**：已完成。`docs/dev-validation.md` 现在明确记录 managed dev compose 的 `redis`、`proxy-pool`、`watchtower-proxy-pool` 职责，Watchtower HTTP API command、label-enable 语义、token 配对、GHCR latest 更新路径，以及 rollback/pause 只能由 operator 显式决定，不能由 smoke runner 或测试自动执行。

**主要完成项**：

- [x] 文档化 `proxy-pool`、`watchtower-proxy-pool` 和 `redis` 的 dev compose 职责。
- [x] 明确 `proxy-pool` 使用 `com.centurylinklabs.watchtower.enable=true`，Watchtower sidecar 使用 `com.centurylinklabs.watchtower.enable=false`。
- [x] 记录 Watchtower command：`--http-api-update --cleanup --label-enable`。
- [x] 明确 dev 默认配置支持 `PROXY_POOL_UPDATE_ENABLED=true`、GHCR latest 镜像和 Watchtower HTTP API。
- [x] README 的 Dev 验证段落补充 dev compose 自更新配置、Watchtower 角色、token 配对和回滚/暂停边界指向。
- [x] 新增 drift guard 断言，覆盖容器角色、labels、Watchtower command、latest 镜像发布关系、rollback/pause operator boundary。
- [x] 更新 `.trellis/spec/integration/testing/config-runbook-drift-check.md`，沉淀新增 runbook 契约。
- [x] `python -m pytest tests\integration\test_l0_config_runbook_drift.py -q` 通过。
- [x] `python -m pytest tests\integration\test_l0_config_runbook_drift.py tests\integration\test_l0_release_status_public_smoke.py -q` 通过。

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
- [x] `gateway-route-debugging` 曾从 Now 移到 `Paused Closeout`；现已按用户恢复要求完成收尾并归档。
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

> 下列为**地图级** Ready：可 `task.py create` 后补全 PRD。建议顺序 **A → B → C**；A 可独立先做。  
> 开工须 `task.py start`，禁止未 start 即写 Now。

### P0-B — `gateway-failure-feedback-v1`

**目标**：网关上游失败后，后续选择能避开同一坏出口（跨请求；尽量跨短重启窗口）。

**非目标**：不做质量推荐产品；不 Resume `revalidation-scheduler-priority` stash。

**验收草稿**：

- [ ] 失败路径单测：同一坏 proxy/xray 在反馈窗口内不再被优先选中
- [ ] 文档说明与 circuit / 进程内 cooldown / Redis 的关系
- [ ] 不扩大到 Dashboard 或 MCP 新工具

### P0-C — `dirty-window-hardening-v1`

**目标**：缩短脏代理可被选中的窗口；消除 `free_pool.max_retry` 配置谎言；Basic 订阅 validate-then-admit 或等价隔离。

**非目标**：大规模关闭 fetcher；自动清理 UI；订阅自动发现扩源。

**验收草稿**：

- [ ] example 与代码默认 `validate_interval` 对齐，或显式注释 tradeoff
- [ ] `max_retry` 接线或删除死字段
- [ ] Basic 入池路径测试覆盖「未验证不进可选池」或隔离队列
- [ ] free pool 仍不进 premium（回归 QualityTier 测试）

## Next

（可用性收敛期 **无** 抢 P0 的 Next。原只读契约见 Later。）

## Later

### P2 — `api-readonly-contract-minimal-v1`

**目标**：把当前真正用于自动验证的只读 API/MCP 字段整理成最小契约，避免重新打开已暂停的完整 `mcp-api-contract-smoke-v2` 范围。

**当前状态**：**不阻塞可用性**；从原 Next/P0 降级为 Later(P2)。仅在 P0-A/B/C 空窗且运维明确需要时开工。

**候选功能**：

- [ ] 定义 `/api/status`、`/api/readyz`、`/api/metrics`、`/api/proxies/scores` 的最小只读字段集合。
- [ ] 定义 MCP `service_status`、`update_status` 和 `explain_proxy_scores` 的最小只读字段集合。
- [ ] 明确不覆盖 mutating tools、不覆盖全部运维入口、不触发 `update_service`。
- [ ] 为后续 runner 和 smoke 任务提供稳定字段清单。

### P1 — `proxy-quality-recommendations-dry-run`

**目标**：基于当前分数和轻量质量历史，给出可解释的清理/降权建议，但默认不修改代理池。

**当前状态**：按用户要求先不推进。历史 planning 已归档到 `.trellis/tasks/archive/2026-07/07-07-proxy-quality-recommendations-dry-run`；**无** active 任务目录。后续仅在用户重新确认后恢复。

**候选功能**：

- [ ] MCP/API 提供 dry-run 建议入口，返回候选代理、原因、预期动作和风险说明。
- [ ] 建议规则同时考虑 score、最近成功率、延迟和连续失败。
- [ ] 默认不删除、不降权、不刷新；未来 apply 入口需单独任务确认。
- [ ] 保留可测试的规则函数，避免 API/MCP adapter 重复计算。

### P2 — `xray-config-dry-run-and-remove`

**目标**：让 xray 运维动作可预检、可回退，减少错误配置直接写入运行态的风险。

**当前状态**：按用户要求先不推进。历史 planning 已归档到 `.trellis/tasks/archive/2026-07/07-07-xray-config-dry-run-and-remove`；**无** active 任务目录。

**候选功能**：

- [ ] xray 配置变更 dry-run 校验。
- [ ] 手动移除单个 xray 节点。
- [ ] 记录移除原因和操作结果。
- [ ] MCP/API 返回结构化错误，便于 Web 和自动化工具展示。

### P3 — `warp-ops-enhancement`

**目标**：增强 WARP endpoint 优选、健康检查和手动运维能力。

**当前状态**：按用户要求先不推进。历史 planning 已归档到 `.trellis/tasks/archive/2026-07/07-07-warp-ops-enhancement`；**无** active 任务目录。

**候选功能**：

- [ ] 完善 WARP instance 状态模型：endpoint、latency、loss、healthy、assigned_at、fail_count。
- [ ] 查询最近 WARP optimizer 扫描结果。
- [ ] 支持手动触发 WARP endpoint 扫描。
- [ ] 支持 endpoint pinning，允许临时禁用 optimizer 覆盖。
- [ ] WARP 健康检查结果进入 Prometheus metrics。
- [ ] 增加 WARP 链式代理端到端测试。

### P1 — `fetcher-validator-quality`

**目标**：继续提升代理来源质量、验证结果可解释性和错误诊断能力（umbrella 收尾视角）。

**当前状态**：主交付已归档到 `.trellis/tasks/archive/2026-07/07-07-fetcher-validator-quality`。源级熔断与验证可观测性已拆分为独立 Done 任务。本地仍保留 stash `wip: paused fetcher circuit work`（Keep-Later），**不要**默认 apply。

**已完成并验证（历史）**：

- [x] 为每个 fetcher 记录最近抓取时间、成功/失败状态、抓取数量、解析数量和错误原因。
- [x] 支持按 fetcher 手动刷新，避免每次全量刷新。
- [x] 验证错误分类：invalid proxy URL、client build failed、timeout、request failed、bad status、body read failed。
- [x] MCP `check_proxy` 返回结构化错误类型。

**暂缓 / 已拆分**：

- [x] 源级熔断 → `fetcher-source-circuit-breaker-mvp`（Done）
- [x] 验证结果可观测性 → `validator-observability-v2`（Done）
- [ ] 残余 stash 仅在用户确认 Resume 时处理

### P3 — `xray-subscription-ops`

**目标**：原 umbrella 任务，已拆分为 `xray-node-lifecycle-mvp`、`subscription-source-ops-mvp` 和 `xray-config-dry-run-and-remove`。后续如需更大的 xray/订阅源管理面板，再恢复该 umbrella。

**候选功能**：

- [ ] 增加 gRPC 重连状态指标。
- [ ] 订阅源新增、删除、启用、禁用。
- [ ] 更完整的 xray/订阅源 Web 管理体验。

## Parking Lot

这些想法暂不承诺实现，等 **L0/L1 可用性闭环**稳定后再评估：

- [ ] 多区域出口调度。
- [ ] Dashboard 高级图表。
- [ ] 基于国家/地区的出口偏好策略。
- [ ] 多租户或访问控制。
- [ ] 管理 API 鉴权。
- [ ] 自动回滚到上一镜像 digest。
- [ ] 外部告警集成，如 Telegram、Webhook、Prometheus Alertmanager。
- [ ] 订阅 GitHub / Telegram / LLM search 自动发现扩源（可用性收敛期冻结）。
- [ ] WARP optimizer pinning / 高级运维面板（与 Keep-Later `warp-ops` 草稿重叠时仍以冻结为准）。

## Trellis 任务创建建议

> 实际开工顺序以本文 §Now / §Ready / §Keep-Later 与 Trellis current 为准。

当前可信排队（可用性优先）：

1. Now：空。下一条从 Ready **P0-B** 经 Trellis start。
2. Ready：**P0-B** `gateway-failure-feedback-v1` → **P0-C** `dirty-window-hardening-v1`（P0-A 已 Done）。
3. Later(P2)：`api-readonly-contract-minimal-v1`（不抢 P0）。
4. Keep-Later 仅在用户明确 Resume 后单开任务；**可用性收敛期默认禁止**恢复 stash。
5. archive `2026-07/` 历史任务保留只读，不删不改正文充当 active。

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
