# Scenario-tiered proxy quality routing

## Goal

网关按**目标场景（routes group）**选择不同**质量档（tier）**的出口顺序：宽松场景优先用 free_pool 等低门槛出口，严格场景只用 WARP/xray 等高档出口且**不降级到 free 脏池**，避免全站一套策略导致要么出网难、要么高价值目标被脏 IP 污染。

## Background

### Live supply (2026-07-17, git_hash=5c7c678)

- 订阅几乎只有 `search` 公开池：~9k encrypted + free_pool 约 1.5k–2.9k
- xray Active 很少；多数 failed 为 `xray validation failed`（HTTP Strict / Cloudflare）
- 公开池质量两极：大量死端口/脏 IP，少量可用节点

### Existing building blocks

| Piece | Location | Today |
|-------|----------|-------|
| Route exits | `route_debug.rs` `RouteExit` | Direct / FreePool / Warp / Xray / NoProxy |
| Known group exit maps | `exits_for_known_group` | `direct` / `free_pool` / `warp` / `xray` 硬编码顺序 |
| Routes YAML | `config/routes.example.yaml`, `Router` | `groups.<name>: [suffix…]` 最长后缀匹配 |
| Unused hook | `Router.scenes` / `scene_for` | 已有字段未接线 |
| Capability prefer | `try_pool_candidates` | OpenAI/ChatGPT 主机偏好 `chat_gpt` 标签 free 节点 |
| Hardcoded domains | `BUSINESS_OVERSEAS_DOMAINS` 等 | 影响 exit 顺序，无显式 tier |

## Requirements

### R1: Quality tiers (D1)

定义三档出口策略（主轴 = **出口类型顺序**，不是纯 score / 来源）：

| Tier | Exit order (D6) | Fail behavior |
|------|-----------------|---------------|
| `any` | FreePool → Warp → Xray → NoProxy | 可借高档出口（D3） |
| `standard` | Xray → Warp → FreePool → NoProxy | 允许 FreePool，但先试加密/WARP |
| `premium` | Xray → Warp → NoProxy | **禁止** FreePool（D2 硬边界） |

另：`direct` 组保持「仅 Direct」语义，不强制塞进三档（配置/已知组特殊处理）。

### R2: Scenario = routes group (D4)

- 继续用 routes YAML 的 host 后缀 → group 匹配
- 扩展 YAML，使每个 group 可声明 `tier`（及可选显式 `exits` 覆盖）
- **向后兼容**：今日 `groups.name: [suffix…]` 列表格式仍可用；列表形式用代码内默认 tier 映射

### R3: Default mapping when tier omitted (D6)

| Group name (if present) | Default tier / behavior |
|-------------------------|-------------------------|
| `direct` | Direct only |
| `free_pool` | `any` |
| `warp` | `premium`（与现 warp 组无 FreePool 对齐） |
| `xray` | `premium` |
| 其他自定义 group | `any`（安全默认，避免未知组抬高导致大面积 NoProxy） |
| 无 routes / general fallback | `any` |

未匹配 host 落在声明了 `default` 的 group；该 group 的 tier 决定行为（example 里 default 在 `direct` 则仍直连）。

### R4: Hard boundary vs borrow (D2/D3)

- **premium**：资源不足 → NoProxy（或错误），**不**落到 FreePool
- **any**：FreePool 空 → 可 Warp → Xray → NoProxy
- **standard**：按表尝试；FreePool 在 Warp/Xray 之后仍允许

### R5: Observability

`RouteDecision` / `route_test` 至少暴露：

- matched group / rule
- resolved `tier`
- ordered exits
- selected exit + skip reasons（已有结构可扩展字段）

### R6: Scope (D5)

- **仅路由层**（`proxy-core` router + route selection；gateway/API 诊断透传）
- **不改** xray 准入、TCP precheck、HTTP Strict 验证、订阅扩源

## Decisions

| # | Decision | Choice | Date |
|---|---|---|---|
| D1 | 质量档定义轴 | 出口类型档（RouteExit 顺序） | 2026-07-17 |
| D2 | 高档不足 | 硬边界：premium 不降 FreePool | 2026-07-17 |
| D3 | 低档 any 兜底 | FreePool → Warp → Xray → NoProxy | 2026-07-18 |
| D4 | 配置形态 | 扩展 routes YAML（兼容旧 groups 列表） | 2026-07-18 |
| D5 | 范围 | 仅路由层 | 2026-07-18 |
| D6 | 默认出口表 + default | 见 R1/R3；general fallback = `any` | 2026-07-18 |

## Acceptance Criteria

- [x] AC1: 配置/默认下，`any` 场景与 `premium` 场景对同一 host 类得到不同 exit 顺序（route 诊断可区分）
- [x] AC2: `premium` 在仅有 FreePool、无 Warp/Xray 时 **不** 选 FreePool（选 NoProxy 或不可用）
- [x] AC3: `any` 在 FreePool 不可用但 Warp 或 Xray 可用时仍可选高档出口
- [x] AC4: 旧版 `groups: { name: [suffix] }` YAML 仍能加载；缺省 tier 映射符合 R3
- [x] AC5: `RouteDecision`（或等价诊断）含 `tier`（或同等字段）
- [x] AC6: `cargo test -p proxy-core`（及相关）与 `cargo clippy -p proxy-core -- -D warnings` 通过

Verified 2026-07-18: `cargo test -p proxy-core` 170 ok; `cargo clippy -p proxy-core -- -D warnings` ok;
focused tests `tiered_route_plans_*` / `legacy_yaml_*` / `reject_premium_*` / `route_decision_serializes_*` ok;
`cargo test -p proxy-api route_test` ok.

## Out of Scope

- xray 双轨准入 / 放宽 Cloudflare 验证
- 按 score/`min_score` 分档（可 follow-up）
- 按订阅来源（机场/公开）分档
- 机场扩源、GitHub/TG 源接入
- 客户端 UI
- 重写 capability 体系（保留现有 ChatGPT 偏好即可）

## Technical Notes

- 主文件：`crates/proxy-core/src/router.rs`, `route_debug.rs`, `config/routes.example.yaml`
- 可能触及：`proxy-api` route_test 序列化、gateway 若直接依赖 exit 计划
- 相关已归档：`07-17-xray-tcp-precheck`, `07-17-xray-active-health-demotion`
