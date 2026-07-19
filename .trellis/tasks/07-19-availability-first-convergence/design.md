# Design: 可用性优先收敛

## 1. Scope

| 做 | 不做 |
|----|------|
| 改写 `docs/ROADMAP.md` 方向、优先级、队列 | 改 `proxy-gateway` / `scheduler` / `route_debug` 行为 |
| 固化 Availability-First 分层与保冻表 | 部署、启服、SSH |
| 排出 P0-A/B/C 地图供后续 `task.py create` | 本任务内实现 P0-A/B/C |
| 1-WIP：metrics 收尾/暂停后再占 Now | Resume Keep-Later stash |

## 2. Artifact boundaries

```text
本任务
  ├── prd.md          需求与 AC
  ├── design.md       本文：队列设计与文案结构
  ├── implement.md    执行清单与验证命令
  └── docs/ROADMAP.md 唯一主要产品 diff

后续任务（不在本 diff）
  ├── reliable-exit-defaults-v1      配置/example/文档
  ├── gateway-failure-feedback-v1    网关→池反馈
  └── dirty-window-hardening-v1      复验/准入/死配置
```

## 3. ROADMAP 目标结构

建议章节顺序（可在现有标题上增量，避免无关大删 Done 历史）：

1. **管理原则**（保留 1-WIP；可加一句 Availability-First 不破坏 1-WIP）
2. **优先级定义**（重写表，见下）
3. **Availability-First（新增）**
   - 目标句
   - 健康定义（三元 AND）
   - L0–L4 分层
   - 保 / 冻 / 后做表
4. **Current Planning Decision**（更新：可用性收敛取代“仅过程债”为当前决策）
5. **Now**（执行期=本任务；收尾规则）
6. **Ready**（P0-A/B/C）
7. **Next / Later / Keep-Later / Parking / Done**（降级平台项；Keep-Later 表保留）

### 3.1 优先级表（目标文案）

| 优先级 | 含义 | 示例 |
|--------|------|------|
| P0 | 阻塞「持续可用」 | 可靠出口默认路径、失败反馈、脏窗口、服务存活信号 |
| P1 | 出口质量与供给 | xray 准入、WARP 健康、复验优先级、free pool 降权 |
| P2 | 观测与运维效率 | metrics 契约、route dry-run、最小只读 API 契约 |
| P3 | 能力扩展 / UI | Dashboard、WARP pinning、订阅自动发现 |

### 3.2 L0–L4

| 层 | 组件 | 策略 |
|----|------|------|
| L0 | Gateway + Redis + 复验调度 + 回退 | 必须稳 |
| L1 | WARP + xray + 干净订阅 | 主供给 |
| L2 | free pool 抓取/评分 | 可选兜底；不进 premium；冻结扩张 |
| L3 | status/readyz/metrics 关键集、route_test、业务 smoke | 服务排障 |
| L4 | Dashboard、完整 MCP 契约、自动发现全家桶 | 冻结 |

### 3.3 Ready 地图（写入 ROADMAP 的最小字段）

**P0-A `reliable-exit-defaults-v1`**

- 目标：example/文档默认出海走 Xray→Warp；`default` 不再误导为 Direct。
- 非目标：不改选路算法本身；不强制用户生产配置自动迁移。
- 验收草稿：`routes.example.yaml` + 说明与 QualityTier premium/standard 一致；README 路由段同步；相关 drift 测试若存在则更新。

**P0-B `gateway-failure-feedback-v1`**

- 目标：网关上游失败后，后续选择能避开同一坏出口（跨请求，最好跨重启窗口）。
- 非目标：不做完整质量推荐产品；不 Resume revalidation stash。
- 验收草稿：失败路径单测；同一坏 proxy 在反馈窗口内不再被优先选中；文档说明与 circuit 关系。

**P0-C `dirty-window-hardening-v1`**

- 目标：缩短脏代理可被选中的窗口；消除 `max_retry` 配置谎言；Basic 订阅 validate-then-admit 或等价隔离。
- 非目标：大规模关 fetcher；自动清理 UI。
- 验收草稿：example 与代码默认 revalidate 对齐或显式注释 tradeoff；死配置接线或删除；Basic 路径测试。

**顺序建议**：A（配置心智）→ B（运行时反馈）→ C（供给清洁）；A 可独立先做。

### 3.4 降级项

| 项 | 处置 |
|----|------|
| `api-readonly-contract-minimal-v1` | 移出抢 P0 的 Next；→ Later(P2) 或标注“不阻塞可用性” |
| metrics 审计 | 收尾进 Done；不再开下一轮观测精修占 Now |
| Dashboard / mcp contract v2 / quality dashboard / fetcher ranking | 保持 Keep-Later，文案加“可用性收敛期不 Resume” |
| 订阅自动发现 / LLM search 扩源 | Parking 或 Later 冻结 |

## 4. 1-WIP 交接设计

```text
metrics (in_progress, 实现已完)
  → commit + archive + ROADMAP Done
  → 或：Keep-Later 暂停仪式（若用户不要求 commit）
然后
availability-first-convergence start
  → 改 ROADMAP
  → finish/archive
  → 下一条从 Ready P0-A start
```

双 Now 禁止。本任务 `task.json` 优先级语义为 **P0（方向）**；若工具无 set-priority，在 ROADMAP 与 notes 写 P0，不必强改 json 字段。

## 5. Compatibility / rollback

- 只改文档与任务元数据 → 回滚 = `git checkout -- docs/ROADMAP.md` + 恢复任务目录。
- 不改变运行中服务行为。
- 不 apply stash，无功能回滚风险。

## 6. Risks

| 风险 | 缓解 |
|------|------|
| metrics 未收尾导致双 Now | implement 第 0 步硬门禁 |
| ROADMAP 大段重写丢历史 | Done/Keep-Later 表保留；增量改 Now/Ready/优先级 |
| P0 地图过细变成假 PRD | 地图只写目标/非目标/验收草稿；完整 PRD 留给子任务 |
| 用户期望本任务改代码 | PRD Out of Scope 已钉死；start 前审阅确认 |

## 7. Success metrics（本任务）

- 任意新 session 只读 ROADMAP 即可回答：现在该做什么、不该做什么、何为健康。
- 下一条代码任务只能是 P0-A/B/C 之一，而非契约/UI。
