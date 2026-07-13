# Implement: 运营清理与分池取用

## Slice 1 — PoolTier enum + status 集成

**文件**：`crates/proxy-core/src/status.rs`

1. 新增 `PoolTier` enum（Stable/Degraded/Minimal/Unstable），`#[serde(rename_all = "snake_case")]`
2. 新增 `PoolTier::from_status(xray_enabled: bool, xray_active: usize, warp_healthy: usize)` 方法
3. 在 `PoolStatus` 中增加 `pub tier: PoolTier` 字段
4. 更新 `collect_pool_status` 签名，接收 xray/warp 参数计算 tier
5. 更新 `collect_service_status` 传递 xray/warp 信息
6. 更新现有测试中 `PoolStatus` 构造（加 `tier` 字段）
7. 新增 `pool_tier_from_status` 单元测试

**验证**：`cargo test -p proxy-core`，`cargo clippy -p proxy-core -- -D warnings`

## Slice 2 — API/MCP status 输出包含 tier

**文件**：`crates/proxy-api/src/routes.rs`，`crates/proxy-mcp/src/lib.rs`

1. 确认 `/api/status` 和 MCP `service_status` 已通过 `ServiceStatus` 序列化输出 tier
2. 无需额外代码（`PoolTier` 通过 `PoolStatus.tier` 自动序列化）
3. 验证 JSON 输出包含 `tier` 字段

**验证**：`cargo test -p proxy-api`，`cargo test -p proxy-mcp`

## Slice 3 — docs/ops-cleanup.md

**文件**：`docs/ops-cleanup.md`（新建）

内容：
1. Cleanup playbook（dry-run → inspect → apply 流程）
2. 推荐参数（min_score=0.35, limit=200）
3. Stale 识别方法
4. Apply 前检查（xray active 不应被 cleanup）
5. 频率建议
6. Noisy fetcher 禁用指南（F4）

## Slice 4 — docs/proxy-usage.md

**文件**：`docs/proxy-usage.md`（新建）

内容：
1. Stable overseas 推荐参数
2. Free pool 查询
3. Domestic 查询
4. MCP/REST 示例

## Slice 5 — config/settings.example.yaml 更新

**文件**：`config/settings.example.yaml`

1. 在海外 profile 示例中增加 tier 语义注释
2. 增加 fetcher 禁用示例注释

## Slice 6 — 最终验证

1. `cargo test --workspace`
2. `cargo clippy --workspace -- -D warnings`
3. `cargo fmt --all -- --check`
