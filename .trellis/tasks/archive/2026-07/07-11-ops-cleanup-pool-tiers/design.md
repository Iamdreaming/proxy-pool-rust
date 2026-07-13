# Design: 运营清理与分池取用

## Boundaries

- **文档为主**：F1/F3/F4 纯文档，不涉及代码变更
- **F2 tier 信号**：在现有 `ServiceStatus` 中增加 `pool_tier` 字段，不改变 Redis 结构或路由逻辑
- **不改路由**：海外出口顺序调整属于 `subscription-xray-overseas` 子任务 F4，本任务仅定义语义

## F1 — Cleanup Playbook

新增 `docs/ops-cleanup.md`，内容：

1. **Dry-run 流程**：`cleanup_low_score_proxies(apply=false)` → 检查 candidates → 决定是否 apply
2. **推荐参数**：`min_score=0.35`（对齐 D2），`limit=200`（分批）
3. **Stale 识别**：`explain_proxy_scores` 中 `trend.recent_samples == 0` 或 `last_checked_at` 超过 1h
4. **Apply 前检查**：确认 candidates 中无 xray active 节点（source=xray 的不应被 cleanup 移除）
5. **频率建议**：手动/按需，暂不自动

## F2 — Tier Semantics

### Tier 定义

| Tier | 含义 | 判定条件 |
|------|------|----------|
| `stable` | 可靠海外出口 | xray active_nodes ≥ 3 **且** WARP healthy ≥ 1 |
| `degraded` | 降级但可用 | WARP healthy ≥ 1 但 xray active < 3 |
| `minimal` | 仅 WARP | WARP healthy ≥ 1，xray 未启用或 0 active |
| `unstable` | 无可靠出口 | WARP 0 healthy 且 xray 0 active |

### 实现方式

在 `status.rs` 的 `ServiceStatus.pool` 中增加 `tier: PoolTier` 字段：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolTier {
    Stable,
    Degraded,
    Minimal,
    Unstable,
}
```

`PoolTier::from_status(xray_active, xray_enabled, warp_healthy, warp_configured)` 计算逻辑：

```
if xray_enabled && xray_active >= 3 && warp_healthy >= 1 → Stable
else if warp_healthy >= 1 && xray_active < 3 → Degraded
else if warp_healthy >= 1 && !xray_enabled → Minimal
else → Unstable
```

### 不做的事

- 不改 Redis sorted set 结构
- 不加 tier label 到 proxy 记录
- 不改路由逻辑（路由调整在 subscription-xray-overseas F4）

## F3 — Default Fetch Guidance

在 `docs/proxy-usage.md` 中记录：

1. **Stable overseas 推荐**：`get_best_proxy(min_score=0.35, max_latency=2000, overseas=true, alive=true)`
2. **Free pool 查询**：`get_proxy(overseas=true)` — 不保证质量
3. **Domestic**：`get_proxy(overseas=false)`
4. **MCP 示例**：每个场景给出 JSON 参数

## F4 — Disable Noisy Free Sources

在 `docs/ops-cleanup.md` 中增加一节：

1. 列出 `/api/fetchers` 返回的 fetcher 状态字段说明
2. 推荐禁用规则：`consecutive_failures >= 5` 或 `validation_survival_rate < 0.05` 的 fetcher
3. 禁用方式：`config/settings.yaml` 中 `pool.fetchers.<id>.enabled: false`
4. 示例 YAML 片段

## Compatibility

- `PoolTier` 是新增字段，不影响现有 API 消费者（JSON 新增 key 被忽略）
- 文档变更无兼容性影响
- 不改配置 schema（fetcher enabled 已有）

## Rollback

- `PoolTier` 字段可安全移除，不影响核心逻辑
- 文档可独立回退
