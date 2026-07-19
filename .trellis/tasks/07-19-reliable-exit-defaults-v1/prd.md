# PRD: 可靠出口默认路径（P0-A）

## Goal

让仓库 **默认示例与文档** 体现「持续出海可用」心智：未匹配主机默认走 **可靠出口**（Xray → Warp，不经 free_pool），国内必要域仍 Direct；不再把 `default → direct` 宣传成唯一/推荐默认。

本任务以 **配置 example + 文档对齐** 为主；**仅**在 default 命中路径增加 GeoIP 分流（R1b），**不改** QualityTier 出口表与非 default 规则。

## Background

- ROADMAP Ready **P0-A** `reliable-exit-defaults-v1`（可用性收敛后的第一条配置/文档向任务）。
- 现状问题：
  - `config/routes.example.yaml` 将 `default` 放在 `direct` → 未匹配海外主机直连，易失败。
  - `settings.example.yaml` **未**声明 `routes_path`，易漏配路由文件。
  - README「路由决策链」仍写「回退链 → 池代理 → WARP → xray」，与 QualityTier（premium/standard 优先 xray/warp）不一致。
- 代码行为（不改）：`routes_path` 有则加载 YAML；无则 GeoIP/硬编码/`any` 回退。组名 `direct` 无 tier → Direct-only；`premium` 永不 free_pool。

## Decisions

| # | 决策 | 选择 | 含义 |
|---|------|------|------|
| D1 | 主 example 姿态 | **overseas-stable** | `default` 落在 `tier: premium` 组，不在 `direct` |
| D1b | 未匹配 + GeoIP | **位置分流** | default 命中且 default 组**非** direct-only 时：有 GeoIP 则国内→Direct、境外→该组 tier 出口；无 GeoIP 则退回组策略。direct-only 的 default（domestic-friendly）仍整组 Direct，不被 GeoIP 改成出海 |
| D2 | 国内域 | **保留 Direct** | `*.cn` 显式在 `direct`；另：GeoIP 判定国内的未匹配主机也 Direct |
| D3 | free_pool | **显式域名 only** | 仅示例性脏流量域；不承担 `default` |
| D4 | 第二 profile | **注释块保留 domestic-friendly** | 需要「未匹配也直连」时可复制注释块 |
| D5 | 算法 | **仅 R1b** | 不改 QualityTier 出口表与非 default 规则；**仅** default 命中增加 GeoIP 分流（`route_debug`） |
| D6 | settings | **补 `routes_path` 示例** | 注释指引复制 routes example |
| D7 | 生产配置 | **不自动迁移** | 不改用户已有 routes；不部署、不启服 |
| D8 | 测试 | **最小** | 单测读取 `routes.example.yaml` 锁定 default≠direct |

## Confirmed Facts

- `Settings.routes_path: Option<String>`；`proxy-server` 仅在 Some 时加载 Router。
- 当前 example：`direct` 含 `*.cn` + `default`；与可用性目标冲突。
- README 路由段与 tier 表不一致。
- QualityTier 契约见 `.trellis/spec/proxy-core/backend/scenario-quality-tiers.md`。

## Requirements

### R1 — `config/routes.example.yaml` overseas-stable 主默认

1. `default` **不在** `direct` 组。
2. 未匹配主机进入 **premium** 组（推荐组名 `overseas`，`tier: premium`，domains 含 `default`）——作为无 GeoIP 时的组策略，且为有 GeoIP 时境外出口表。
3. `direct` 仅国内/直连域（至少 `*.cn`）。
4. free_pool 仅显式域名示例，`tier: any`。
5. 文件头注释：主 profile = overseas-stable；GeoIP 国内未匹配→Direct；并保留 **domestic-friendly** 注释示例。
6. example 不得违反 premium+free_pool 禁止规则。

### R1b — default 命中的 GeoIP 分流（最小代码）

1. 有 Router 且命中 `default` 时，在 domain helpers 之后：
   - 若 default 组为 **direct-only** → 仍 Direct（domestic-friendly 不变）。
   - 否则若 **GeoIP 可用** → 国内 Direct；境外使用 **default 组** 的 tier/exits（overseas-stable 下为 premium）。
   - 否则 → 现有 `resolve_group_policy(default 组)`。
2. 非 default 的显式规则仍完全由组策略决定（不插入 GeoIP）。
3. 覆盖单测：default+premium+geoip 国内/境外；default+direct-only 不被 GeoIP 改写。

### R2 — `config/settings.example.yaml`

1. 增加 `routes_path` 注释/示例指引。
2. 注释提示：premium 依赖 WARP/xray，否则可能 502。

### R3 — README（及可选交叉引用）

1. 重写「路由决策链」为 QualityTier 真实顺序。
2. 写明高价值/默认出海用 premium；free pool 不进 premium。
3. 指向 `config/routes.example.yaml`。

### R4 — ROADMAP

1. start 后 Now = 本任务；完成后 P0-A → Done，Now 置空。
2. 不 Resume Keep-Later；不抢做 P0-B/C。

### R5 — 回归

1. 自动化测试加载 example 全文：`default` 组非 `direct` 且为 premium；`foo.cn` → direct。
2. `cargo test -p proxy-core` 路由相关通过。

## Acceptance Criteria

- [x] **AC1** `routes.example.yaml`：`default` ∈ premium 组；不在 `direct`。
- [x] **AC2** `*.cn` 仍 Direct-only。
- [x] **AC3** free_pool 不承载 `default`；premium 组无 free_pool。
- [x] **AC4** `settings.example.yaml` 可见 `routes_path` 指引。
- [x] **AC5** README 路由段与 QualityTier 一致，并写明 default+GeoIP 国内直连/境外可靠出口。
- [x] **AC6** 自动化测试锁定 example 可加载 + default 非 direct。
- [x] **AC7** 选路仅增加 R1b 的 default+GeoIP 行为；不改非 default 规则、不改 tier 表本身。
- [x] **AC8** domestic-friendly（default∈direct）仍整组 Direct，单测保证不被 GeoIP 改写。
- [x] **AC9** ROADMAP：P0-A 完成进 Done；遵守 1-WIP。
- [x] **AC10** default+非 direct-only+GeoIP：国内→Direct，境外→premium（或组 tier）出口顺序。

## Out of Scope

- P0-B / P0-C
- 修改实机生产 routes、启服、SSH、`update_service`
- 改 QualityTier 出口表枚举或硬编码业务域表内容
- Dashboard / MCP 新工具
- 强制 `xray.enabled: true` 为代码默认（仅可注释建议）

## Open Questions

已关闭：用户确认未匹配应按 IP 位置（中国 Direct）；D1 仍为 premium 作为境外/无 GeoIP 默认组。

## Notes

- 审阅 `design.md` + `implement.md` 后再 `task.py start`。
