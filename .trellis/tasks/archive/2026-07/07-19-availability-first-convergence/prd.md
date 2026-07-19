# PRD: 可用性优先收敛（ROADMAP 重排与 P0 拆分）

## Goal

把项目执行主线从「代理池平台 / 观测与过程债」收敛为单一目标：

> **客户端连 Gateway `:9080`，在需要的时段内尽量一直有可用出口。**

本任务交付的是 **方向与队列收敛**（文档 + 任务地图 + 1-WIP 交接），**不**在本任务内改网关/调度业务代码。后续代码型 P0 以独立 Ready 任务承接。

用户价值：恢复“一次只做一件且对可用性有贡献的事”，避免 free pool / Dashboard / 契约 smoke 继续抢 Now。

## Background

- 用户目标唯一：持续正常使用代理。
- 架构评审结论：**分层合理，不推倒重来**；问题在默认策略与优先级，不在 crate 边界。
- 心智切换：
  - 旧：免费代理抓取平台 + 运维产品
  - 新：个人稳定代理出口网关（**xray + WARP 主用**，free pool 可选兜底）
- 线上端口不通已确认为 **用户主动停服**，不纳入本任务缺陷。
- 当前另有 `07-19-metrics-low-cardinality-audit-v1` 为 `in_progress`（ROADMAP Now），交付深度 A 已完成待 commit/archive；本任务 **start 前必须先完成其收尾或显式暂停**，以遵守 1-WIP。

## Decisions

| # | 决策 | 选择 | 含义 |
|---|------|------|------|
| D1 | 本任务交付物 | **文档收敛 + P0 地图** | 改 `docs/ROADMAP.md`（及必要交叉引用）；不改 gateway/scheduler 逻辑 |
| D2 | 北极星健康定义 | **三元 AND** | ① gateway 进程/端口可用 ② `pool.tier` ≥ `minimal`（目标 `stable`）③ 业务 smoke 通过 |
| D3 | 主供给 | **xray + WARP** | free pool 降为 L2 辅助；不进 premium |
| D4 | 默认路由方向 | **出海优先表述** | ROADMAP 写清：未匹配海外不得默认当 Direct；example 调整由 **P0-A** 执行 |
| D5 | free pool | **保留代码，冻结扩张** | 不删 free pool；冻结来源排名/质量 dashboard 等投入 |
| D6 | 子任务拆分 | **本任务只出地图** | P0-A/B/C 在 ROADMAP Ready；代码任务另 `task.py create`，本任务可不建 children |
| D7 | Keep-Later stash | **不 Resume** | 禁止默认 apply/drop；可用性相关也不从 stash 恢复，新开干净任务 |
| D8 | metrics 任务 | **先收尾再切 Now** | start 本任务前：metrics commit+archive 或 Keep-Later 暂停仪式 |
| D9 | 优先级定义 | **P0=持续可用** | ROADMAP 优先级表以可用性阻塞为 P0；过程/契约/UI 降级 |

## Confirmed Facts（仓库可证）

### 关键路径

- Gateway `:9080`：HTTP CONNECT / SOCKS5 → `UpstreamSelector` → FreePool / Warp / Xray / Direct / NoProxy
- QualityTier：`any` / `standard` / `premium`（premium 禁止 free_pool）
- `pool.tier`：`stable` = xray active≥3 且 WARP healthy≥1（只读）
- `xray.enabled` 默认 false；订阅 example 默认注释
- example routes：`default` → `direct`（与持续出海目标冲突）

### 已知脆弱点（后续 P0，非本任务实现）

| 点 | 说明 |
|----|------|
| 网关失败不回写 Redis | 仅进程内 300s cooldown |
| `free_pool.max_retry` 未接线 | 配置谎言 vs 硬编码候选上限 |
| revalidate example=600s | 脏窗口偏长 |
| 订阅 Basic 可未验入库 | 污染 free pool |
| scheduler/API/gateway 同命运 | API 问题可拖垮网关 |

### 收敛前队列

- Now：`metrics-low-cardinality-audit-v1`（P2，待收尾）
- Next：`api-readonly-contract-minimal-v1`
- Keep-Later：7 stash（Dashboard、mcp contract、revalidation 等）

## Requirements

### R1 — 北极星与分层写入 ROADMAP

1. `docs/ROADMAP.md` 增加 **Availability-First** 节，写明：单一目标、健康定义 D2、L0–L4 分层。
2. 更新「优先级定义」：P0 = 阻塞持续可用；观测/UI/契约不得默认占 P0。

### R2 — Now / Ready / Next 重排

1. 执行期 Now = 本任务；metrics 已不在 Now。
2. 收尾后 Now 置空，或交给已 start 的下一条 P0（完成仪式写清）。
3. Ready 地图：
   - **P0-A** `reliable-exit-defaults-v1`：默认出海走 xray/WARP（routes/settings example + 文档）
   - **P0-B** `gateway-failure-feedback-v1`：网关失败影响后续选择
   - **P0-C** `dirty-window-hardening-v1`：复验间隔、Basic 准入、死配置清理
4. `api-readonly-contract-minimal-v1` 降为不抢 P0（Later 或低优先级 Next）。
5. 冻结：Dashboard、mcp-api-contract-smoke-v2、订阅自动发现扩源、质量 dashboard、fetcher 来源排名 — Keep-Later/Parking，不 Resume。

### R3 — 保 / 砍 / 冻清单

ROADMAP 含可执行表：保（L0/L1/最小观测）、冻（L4）、后做（P0-A/B/C）。

### R4 — 1-WIP 与 metrics 交接

1. 改 ROADMAP Now 前：metrics 已 archive 或已暂停仪式。
2. 禁止双 Now。
3. 本任务 diff 以 `docs/ROADMAP.md` + 本任务目录为主。

### R5 — P0 地图字段

每条 P0 至少：一句话目标、非目标、验收草稿、建议顺序（A 先；B/C 次之）。

## Acceptance Criteria

- [x] **AC1** ROADMAP 含 Availability-First 目标句 + 健康定义（gateway / tier / 业务 smoke）。
- [x] **AC2** 优先级定义已改为可用性优先。
- [x] **AC3** Now 与 Trellis 一致且最多 1 条；metrics 不再占用 Now。
- [x] **AC4** Ready 出现 P0-A / P0-B / P0-C 地图项（目标+验收草稿）。
- [x] **AC5** 平台化项已标注冻结或不抢 P0；未 Resume stash。
- [x] **AC6** 本任务 diff 无 gateway/scheduler/xray 业务逻辑实现。
- [x] **AC7** 保/砍/冻表可供后续 session 直接执行。
- [x] **AC8** 规划产物含 `prd.md` + `design.md` + `implement.md`；jsonl 已 curated。

## Out of Scope

- 实现网关失败回写、改 routes 默认代码、改 revalidate 间隔代码
- 启停线上服务、SSH、`update_service`
- stash apply/drop
- 删除 free pool 或大规模关 fetcher（仅策略声明）
- Dashboard / MCP 新工具
- 重做 metrics 审计 scope

## Open Questions

无阻塞。留给 P0-A：生产 routes 是否已与 example 不同；业务 smoke 目标站是否固化。

## Notes

- 审阅通过后再 `task.py start`。
- 代码 P0 为后续独立可归档交付，不在本 parent 内实现。
