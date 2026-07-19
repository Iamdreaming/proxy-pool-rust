# Prometheus 指标低基数审计

## Goal

系统性审计 `/api/metrics` 的业务指标与 label，确认质量 / 路由等现有指标保持低基数，并把允许 label 集合与禁止高基数字段固化为可回归的测试与文档（交付深度 **A**：清单 + 白名单/负向测试 + 既有 spec 扩节），防止后续新增指标破坏约束。

## Background

- ROADMAP Ready：`metrics-low-cardinality-audit-v1`（P2）。
- 质量与路由指标已在先前任务落地；本任务不是新增业务能力，而是把低基数约束从“约定”变成“可检查契约”。
- 运维依赖 no-SSH 的公开 HTTP `/api/metrics` 做观测；label 一旦带上代理地址、完整 URL、原始错误等，会污染 Prometheus 时序并拖垮抓取。

## Confirmed Facts（仓库可证）

### 渲染入口

| 来源 | 函数 | 装配点 |
|------|------|--------|
| 池 / 质量 / 依赖 | `proxy_core::status::render_prometheus_metrics` | `proxy-api` `routes.rs` `metrics` handler |
| 网关路由尝试 | `GatewayRouteMetrics::render_prometheus` via `UpstreamSelector::render_gateway_metrics` | 同上，追加到同一响应 |

`GET /api/metrics` 当前**仅**拼接上述两段；无第三渲染源。

### 现有指标与 label（完整清单）

**无 label（标量 gauge）**

| Metric | 含义 |
|--------|------|
| `proxy_pool_tier` | 0–3 出口可靠性档位 |
| `proxy_quality_recent_samples_total` | 最近验证样本数 |
| `proxy_quality_recent_success_rate` | 最近成功率（无样本时 0.0） |
| `proxy_quality_recent_failures_total` | 最近失败数 |
| `proxy_quality_stale_proxies_total` | 过期代理数 |
| `proxy_quality_stale_after_seconds` | 过期阈值 |
| `proxy_redis_ready` | Redis 就绪 0/1 |
| `proxy_warp_instances_configured` | WARP 配置实例数 |
| `proxy_warp_instances_healthy` | WARP 健康实例数 |
| `proxy_xray_active_nodes` | xray 活跃节点数 |
| `proxy_xray_failed_nodes` | xray 失败节点数 |
| `proxy_uptime_seconds` | 进程 uptime |

**有限枚举 / 有界 label**

| Metric | Label | 允许值（代码硬编码） |
|--------|-------|----------------------|
| `proxy_pool_size` | `protocol` | `http`, `https`, `socks5`, `total` |
| `proxy_quality_score_bucket` | `bucket` | `untested`, `poor`, `fair`, `good`, `excellent` |
| `proxy_quality_retention_candidates` | `decision` | `below_min_score`, `hard_failure_evict` |
| `proxy_quality_failure_reasons_total` | `reason` | 归一化集合；输出截断 top `MAX_FAILURE_REASON_METRICS=5` |
| `proxy_gateway_route_attempts_total` | `protocol` | `http_connect`, `socks5`, `other` |
| 同上 | `exit` | `direct`, `free_pool`, `warp`, `xray`, `no_proxy` |
| 同上 | `status` | `success`, `failure`, `unavailable` |

Gateway 计数器固定展开 `3 × 5 × 3 = 45` 个 series（`METRIC_CELL_COUNT`），不随请求 host/代理地址增长。

### 失败原因归一化

`normalize_failure_reason`（`status.rs`）将任意错误文本映射到：

`unknown` | `timeout` | `bad_status` | `body_read_failed` | `invalid_proxy_url` | `client_build_failed` | `request_failed` | `circuit_open` | `validation_failed` | `other`

既有单元测试 `failure_reason_normalization_is_bounded` 已覆盖 URL / `host:port` 输入。

### 明确不存在的 Prometheus 指标（截至当前代码）

- **fetcher**：无 `proxy_fetcher_*`。
- **release**：`ServiceStatus.release` / `git_hash` 在 status/MCP 中，**未**渲染为 Prometheus 指标。

### 既有测试与缺口

- 已有：quality 渲染 happy-path、reason 归一化、gateway 三维 label 抽样断言。
- 缺口：完整允许 label 白名单锁死；gateway 固定 45 series；渲染输出负向“不得含原始高基数子串”；spec 全量清单章节。

## Requirements

### R1 — 全量指标清单

- 以代码为权威，在 `.trellis/spec/proxy-core/backend/quality-guidelines.md` 扩「Prometheus Low-Cardinality Contract」节，写入完整指标+label 清单。
- 清单区分：无 label 标量、有限枚举 label、动态但有界 label（top-N reason）。

### R2 — 高基数禁止规则

禁止作为 Prometheus **label 值**出现：

- 代理地址 / host:port / dedup key
- 完整 URL、订阅内容、容器动态 ID
- 原始错误字符串、未归一化 free-form 文本
- 请求目标 host（gateway 不得用目标域名做 label）
- git hash、镜像 digest、任意无限增长标识（若未来加 release 指标，不得用 hash 做 label）

### R3 — 允许 label 白名单 + 测试

为现有带 label 指标用自动化测试锁死允许值集合（见 Confirmed Facts 表）：

- 每个 label 只能取白名单内值；
- gateway 固定 45 条 `proxy_gateway_route_attempts_total{...}` sample 行；
- failure reason 只能来自归一化集合，且 top-N ≤ 5；
- 合成含 URL/地址/raw error 的 quality 输入时，渲染结果 label 仍为有界 reason，输出不得包含原始高基数子串。

### R4 — 回归文档

- 更新 quality-guidelines（主契约）；README 仅在 metrics 描述不足时补一句“低基数 label”。
- **不**新增 fetcher / release Prometheus 指标；spec 写明“当前无；若新增必须遵守 R2/R3”。
- **不**抽取跨模块共享测试 helper 框架（交付深度 A，拒绝 B）。

### R5 — 发现违规时的处理

- 审计中若现有实现违反 R2/R3，本任务内做最小修复；不得带着违规完成。
- 不扩大到新业务指标设计（除非仅为消除违规所必需）。

## Acceptance Criteria

- [x] AC1：`quality-guidelines.md` 含与代码一致的 `/api/metrics` 指标+label 清单与禁止规则。
- [x] AC2：自动化测试锁定现有带 label 指标的允许值集合，含 gateway 45 series。
- [x] AC3：failure reason 归一化 + 渲染负向测试：输入含 URL / host:port / 长错误文本时，label 有界且 metrics 文本不含原始子串。
- [x] AC4：spec 明确 fetcher/release 当前无 metrics；未来新增必须遵守同一低基数规则。
- [x] AC5：`cargo test -p proxy-core`（及若改 API 则 `proxy-api`）与 `cargo clippy -p proxy-core -- -D warnings` 零失败；`cargo fmt` 干净。
- [x] AC6：无业务功能回退；不触碰 Keep-Later stash；不 SSH；不默认 push。

## Out of Scope

- 新增 fetcher / release / WARP optimizer 等 Prometheus 指标实现。
- 共享测试 helper 抽象库（交付深度 B）。
- 外部告警、histogram/summary、`prometheus` crate。
- `/api/status` 或 MCP 字段契约变更（留给 `api-readonly-contract-minimal-v1`）。
- 改 exposition 格式。

## Decisions

| ID | 决策 | 选择 |
|----|------|------|
| D1 | 交付深度 | **A**：清单 + 白名单/负向测试 + quality-guidelines 扩节 |
| D2 | 清单落点 | 扩 `quality-guidelines.md`，不新建独立 metrics 文件 |
| D3 | fetcher/release metrics | 本任务不实现，只文档约束 |
| D4 | 共享 helper | 不做 |

## Open Questions

无（规划可进入 design/implement 与 start 评审）。
