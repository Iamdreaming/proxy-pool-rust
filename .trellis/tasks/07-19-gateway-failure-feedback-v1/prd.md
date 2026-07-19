# PRD: Gateway failure feedback v1

## Goal

网关上游连接/转发失败后，**后续请求**在反馈窗口内不再优先选中同一坏出口（free_pool proxy / xray / warp），减少「连挂 → 502 → 立刻再选同一坏点」的抖动；在 v1 约束下尽量覆盖**短重启**窗口（若选型包含耐久层）。

**用户价值**：持续可用 — 坏出口被短暂隔离，健康出口有机会被选中。

## Background (现状，非目标)

- 同请求内 fallback 已存在：`http_connect` / `socks5` 遍历 `upstream_candidates`，失败则 `record_upstream_attempt(Failure)` 再试下一候选。
- **跨请求、进程内** cooldown 已存在（约 300s）：
  - free_pool：`pool_proxy_failed_until`（`dedup_key`）
  - xray：`xray_failed_until`（`local_socks5_port`）
  - warp：`WarpBalancer::mark_failed` + `gateway_failed_until`
- Redis `circuit_open` / `fail_count` / score 由**验证调度 / 运维**写入，**网关数据面不写**。
- 重启后进程内 map 清空 → 坏出口可立刻再被选中（P0-B 主要缺口之一）。
- 前序任务 `gateway-http-connect-fallback-v1` 曾明确 **不写 Redis、不改 score**（R5）；P0-B 若做耐久层需**有意识地**收窄放开，而非全面走 `mark_failed_with_circuit`。

## Scope

### In scope

1. 明确并固化「网关失败 → 后续选择避开」的契约（free_pool / xray；warp 维持现有进程内行为除非设计另定）。
2. 按选定方案实现反馈窗口（进程内 hardened **或** 短 TTL Redis cooldown；**不**默认走 score/circuit 产品路径）。
3. 选择路径单测/集成风格测试：同一坏 proxy/xray 在窗口内不被优先再选。
4. 文档：gateway cooldown ↔ Redis circuit ↔ xray 控制面 demotion 的关系与边界。

### Out of scope (非目标)

- 质量推荐产品 / Dashboard / MCP 新工具
- Resume `revalidation-scheduler-priority` 或任何 Keep-Later stash
- 改 score 公式、min_score、fetcher 扩源、P0-C dirty-window / `max_retry` / Basic 入池
- 把网关失败默认打进 `mark_failed_with_circuit`（Option C，扩大 blast radius）
- WarpChain 完整失败反馈（可记 follow-up）
- 多副本网关全局一致性的完整方案（v1 以单进程 + 可选 Redis TTL 为上限）

## Requirements

### R1 — 跨请求避开坏出口

在反馈窗口内，对 free_pool `dedup_key` 与 xray 身份（见 R3），`select_with_trace` / 等价选择不得将该出口作为**优先**候选（允许耗尽后回退到 NoProxy 等既有链末端行为，与现网一致）。

### R2 — 成功清除

同一出口在网关路径 **Success** 后，应清除其反馈冷却（与现 `record_upstream_attempt(Success)` 语义一致；耐久层若存在则同步清除或允许自然过期，设计中二选一并写清）。

### R3 — 身份键

| 出口 | 身份键（v1） | 备注 |
|------|----------------|------|
| free_pool | `proxy.dedup_key()` | 稳定 |
| xray | 优先稳定节点身份；若仅有 `local_socks5_port` 须在 design 注明重启/换绑局限 | 与现实现一致处可保留 port，但文档必须写清 |
| warp | 现有 instance id 进程内 | v1 不强制 Redis |

### R4 — 与 circuit / 验证面解耦

- 网关反馈**不得**替代或破坏 Redis circuit 语义。
- v1 **禁止**默认调用 `mark_failed_with_circuit` / 改 ZSET score 作为主反馈（除非后续单独决策推翻）。
- 验证调度仍可独立 trip circuit；选择层继续尊重 `circuit_open`。

### R5 — 耐久性（D1 已锁定）

| 选项 | 含义 | 满足「短重启」 |
|------|------|----------------|
| A 进程内 hardened | 保留 map + 测试 + 文档 | 否 |
| **B 短 TTL Redis cooldown（已选）** | 失败写 `SETEX`/等价 TTL key；选择时过滤；TTL≈现有 300s；保留进程内 map | 是（同 Redis） |
| C 网关 → circuit/score | 复用 `mark_failed_with_circuit` | **v1 不采纳** |

**D1 = B**（用户 2026-07-19 确认）。

### R6 — 可观测与文档

- 任务内文档（design + 必要 README/spec/ROADMAP AC 勾选）说明三层关系：
  1. 网关 cooldown（进程内 / Redis TTL）
  2. Redis circuit（验证/运维）
  3. xray outbound 主动健康 demotion（控制面）
- 不新增 MCP/API 工具表面。

### R7 — 回归边界

- QualityTier / premium 永不 free_pool 的既有测试保持绿。
- 同请求 fallback 行为不回退。
- 不启用不相关 stash、不 SSH、不默认 `update_service`。

## Acceptance Criteria

- [x] **AC1** free_pool：process map + Redis put on Failure；`try_pool` 过滤 process OR redis（单测覆盖 process helper + `gateway_cooldown_blocks` + key 格式）。
- [x] **AC2** xray：同上，port 身份 + `try_xray` 过滤。
- [x] **AC3** Success 清除 process map + best-effort Redis DEL（代码路径）。
- [x] **AC4** D1=B：选择路径读 Redis cooldown；无 process map 时仅 `is_gateway_*_cooling_down==true` 仍 skip（逻辑+fail-open 单测；无 live Redis 集成 harness 时以 API+filter 契约锁定）。
- [x] **AC5** `xray-route-eligibility.md` 三层表更新；design 文档化 circuit/demotion 边界。
- [x] **AC6** `route_debug::` / `store::` tests 绿；`clippy -p proxy-core --lib -D warnings` 绿。
- [x] **AC7** 无 Dashboard/MCP 新工具；无 `mark_failed_with_circuit` 网关路径。

## Design Decisions

| ID | Status | Choice |
|----|--------|--------|
| D1 | **Locked** | **B** 短 TTL Redis cooldown（≈300s）；进程内 map 保留；Redis 读失败 fail-open |
| D2 | Locked | free_pool + xray 写 Redis；warp 仅进程内 |
| D3 | Locked | 不写 score；不 trip circuit from gateway |
| D4 | Locked | TTL 对齐现有 300s 常量；v1 不新增 settings 字段 |
| D5 | Locked | xray 身份键 = `local_socks5_port`（文档注明换绑局限） |

## Risks

| Risk | Mitigation |
|------|------------|
| 目标站 5xx 被当成出口坏 | 仅连接/上游建立失败反馈（与现 `record_upstream_attempt` 调用点一致）；不解析业务 HTTP 体 |
| xray port 换绑 | design 注明；可选后续改节点 id |
| Redis 故障 | 选择路径 Redis cooldown 读失败时 fail-open（仍可用进程内 map）— design 锁定 |
| 与旧 R5 冲突 | PRD 显式 supersede「禁止一切 Redis」为「允许仅 cooldown TTL keys」 |

## Notes

- 地图来源：`docs/ROADMAP.md` Ready P0-B。
- 复杂任务：须 `design.md` + `implement.md` + context jsonl，**`task.py start` 后**方可改业务代码。
- 1-WIP：本任务 start 前 Now 保持空。
