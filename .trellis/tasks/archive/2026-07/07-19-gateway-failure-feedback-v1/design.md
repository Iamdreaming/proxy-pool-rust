# Design: gateway-failure-feedback-v1

## 1. Scope

| 做 | 不做 |
|----|------|
| free_pool / xray 网关失败 → 短 TTL Redis cooldown + 保留进程内 map | 网关写 `mark_failed_with_circuit` / 改 score |
| 选择路径同时尊重进程内 + Redis cooldown | Dashboard / MCP 新工具 |
| Success 清除进程内 + Redis key | Resume revalidation-scheduler stash |
| 集成风格单测 + 三层文档 | P0-C dirty-window / max_retry |
| warp 维持现有进程内 `mark_failed` | WarpChain 完整反馈 |
| Redis 读失败 fail-open（仍有进程内） | 多副本强一致产品化 |

**D1 已确认：B — 短 TTL Redis cooldown（≈300s），不写 score/circuit。**

## 2. Background

现状（`route_debug.rs`）：

```text
record_upstream_attempt(Failure):
  Proxy  → pool_proxy_failed_until[dedup_key] = now+300s
  Xray   → xray_failed_until[port] = now+300s
  Warp   → balancer.mark_failed(id)
select:
  try_pool / try_xray skip process-local cooldown
restart:
  maps empty → 坏出口立刻可再选  ← P0-B 主缺口
```

网关 handlers 已调用 `record_upstream_attempt`；**接线点不变**，扩展该函数与过滤逻辑即可。

## 3. Architecture

```text
                    ┌─────────────────────────────┐
  Failure ─────────►│ record_upstream_attempt     │
                    │  1. process map (sync)      │
                    │  2. Redis SETEX (best-effort)│
                    └─────────────────────────────┘
  Success ─────────► clear map + DEL key

  select try_pool / try_xray:
    skip if process cooldown OR redis cooldown (fail-open on redis err)
```

### 3.1 Redis key contract

| Kind | Key | Value | TTL |
|------|-----|-------|-----|
| free_pool | `gateway:cooldown:proxy:{dedup_key}` | `"1"` | 300s |
| xray | `gateway:cooldown:xray:{local_socks5_port}` | `"1"` | 300s |

- 不与 `proxies:{protocol}` ZSET 混用。
- **不**序列化 Proxy JSON；仅存在性 = 冷却中。
- TTL 对齐 `POOL_PROXY_FAILURE_COOLDOWN` / `XRAY_FAILURE_COOLDOWN`（v1 不新增 settings 字段）。

### 3.2 Identity (xray)

v1 继续用 **`local_socks5_port`**（与现 `Upstream::Xray` 与进程内 map 一致）。

局限（文档必写）：

- 进程重启 + xray 重新绑端口 → 旧 port key 可能残留至 TTL，新 port 无冷却（可接受：短窗口误伤旧 port；新 port 靠验证/健康面）。
- 后续可升级为节点 tag/id（out of scope）。

### 3.3 Fail-open / fail behavior

| 操作 | Redis 错误 |
|------|------------|
| SETEX on Failure | log debug/warn；进程内 map 仍生效 |
| DEL on Success | log；进程内已清 |
| EXISTS/GET on select | 视为 **不在冷却**（fail-open），避免 Redis 故障导致全池不可选 |

### 3.4 WARP

保持 `WarpBalancer` 进程内 300s + healthy 标志。单进程网关足够；不做 Redis warp key（D2）。

### 3.5 Layering vs circuit / demotion

| Layer | Writer | Reader | Lifetime |
|-------|--------|--------|----------|
| Gateway cooldown (this task) | 数据面 `record_upstream_attempt` | `try_pool` / `try_xray` | process + Redis TTL 300s |
| Redis circuit | scheduler / `mark_*_with_circuit` | store filters + eligibility | member JSON + recovery 600s |
| Xray active demotion | proxy-xray outbound_sync | pool state Active→torn down | control plane |

Gateway cooldown **does not** trip circuit or demote Active.

## 4. Code shape

### 4.1 Preferred placement

**Thin helpers on `ProxyStore`**（已有 `conn` / `set_ex` 先例在 geoip）或 **同 crate 小模块 `gateway_cooldown.rs`** 接受 `MultiplexedConnection`：

```rust
// Conceptual API — names may match implement style
pub async fn put_gateway_proxy_cooldown(&self, dedup_key: &str, ttl_secs: u64) -> anyhow::Result<()>;
pub async fn clear_gateway_proxy_cooldown(&self, dedup_key: &str) -> anyhow::Result<()>;
pub async fn is_gateway_proxy_cooling_down(&self, dedup_key: &str) -> anyhow::Result<bool>;

pub async fn put_gateway_xray_cooldown(&self, port: u16, ttl_secs: u64) -> anyhow::Result<()>;
pub async fn clear_gateway_xray_cooldown(&self, port: u16) -> anyhow::Result<()>;
pub async fn is_gateway_xray_cooling_down(&self, port: u16) -> anyhow::Result<bool>;
```

实现：`SET EX` / `DEL` / `EXISTS`（或 `GET`）。

**不**放进 `circuit.rs`（语义不同）。**不**改 gateway crate handlers。

### 4.2 `record_upstream_attempt` changes

```text
Proxy Failure:
  insert process map
  store.put_gateway_proxy_cooldown(key, 300).await  // ignore err after log
Proxy Success:
  remove process map
  store.clear_...(key)
Xray: same with port
```

### 4.3 Selection filter changes

`try_pool_candidates` / `try_xray`：

```text
if process_cooldown(key) { skip }
if store.is_gateway_*_cooling_down(key).await.unwrap_or(false) { skip }
```

注意：select 路径可能对 top-k 多 key 查询 → v1 可：

- **简单**：每候选一次 EXISTS（limit=8 / xray 候选通常更少）；或
- **批量**：`MGET` 预取（若实现成本低优先 MGET）

优先正确性；候选上限已小（`FREE_POOL_CANDIDATE_LIMIT=8`）。

### 4.4 Supersede prior R5

`gateway-http-connect-fallback-v1` R5「must not write Redis」→ 本任务 **有限 supersede**：仅允许 **cooldown TTL keys**，仍禁止 score/circuit 写入。

## 5. Tests

| Case | How |
|------|-----|
| AC1 free_pool skip after Failure | unit with store mock or redis test harness used by store tests |
| AC2 xray skip after Failure | same |
| AC3 Success clears | put then success → selectable |
| AC4 restart / new selector | only Redis key set (no process map) → still skip |
| pure helpers | existing process-map tests remain |
| fail-open | optional: inject error → still selects when only redis fails and no process map |

Prefer现有 `store` 测试模式（若有 redis 或 fake）。若无集成 redis，至少：

1. 单元测 store cooldown helpers（mock conn 或 `redis` testcontainer 若项目已有）；
2. 单元测「过滤逻辑」用注入的 `is_cooling` 谓词 / 直接测 helpers + 手动填 map。

最低线：不降低现有 `route_debug` / `store` 测试绿。

## 6. Docs

- Update `xray-route-eligibility.md` complementary table: gateway cooldown = process **+ Redis TTL**.
- Task design 本节 + ROADMAP P0-B AC 勾选（归档时）。
- Optional: `proxy-core` quality-guidelines 一行 pointer（若 index 有 routing 节）。

## 7. Rollout / rollback

- 纯逻辑 + Redis key；无 schema 迁移。
- Rollback：回退代码；旧 key 300s 自过期。
- 不要求 `update_service` 作为验收默认；本地 test/clippy 足够进 archive，部署按用户另指令。

## 8. Open risks (accepted)

- xray port identity 跨重启不完美 — 文档化。
- 目标站拒绝 vs 出口坏：仅连接失败点反馈（现有调用点），不解析 HTTP 业务码。
