# Design: metrics-low-cardinality-audit-v1

## Architecture / Boundaries

- **Owner crate**：`proxy-core`（指标渲染与归一化逻辑）。
- **Assembler**：`proxy-api` `metrics` handler 仅拼接两段字符串，不引入新 label。
- **Spec owner**：`.trellis/spec/proxy-core/backend/quality-guidelines.md` 新增/扩展 Prometheus 低基数契约节；`index.md` 描述句可顺带点出 metrics 契约。
- **不改**：gateway 热路径 record API、status JSON 形状、MCP 工具。

```text
ServiceStatus ──► render_prometheus_metrics ──┐
                                              ├──► GET /api/metrics
GatewayRouteMetrics ──► render_prometheus ────┘
```

## Contracts

### Low-cardinality invariants

1. Label 值必须来自**编译期固定集合**或 **归一化后的有界集合**。
2. Gateway series 数恒为 `METRIC_CELL_COUNT = 45`，全量展开（含 0 值）。
3. `proxy_quality_failure_reasons_total` 最多 5 个 reason series；reason ∈ 归一化集合。
4. 禁止 R2 所列高基数字段进入任何 label 值或作为“伪 label”拼进 metric 名。

### Allowlist (authoritative for tests)

见 `prd.md` Confirmed Facts 表。测试应把该表编码为常量数组，解析 exposition 中的 `key="value"` 做集合包含断言，而不是只 `contains` 个别 happy 行。

### Failure reason path

```text
sample.error (free text)
  → normalize_failure_reason → &'static str (bounded)
  → top_failure_reasons (truncate 5)
  → render label reason="..."
```

负向测试应构造 `QualityStatus.top_failure_reasons` 或走 `normalize_failure_reason` + 渲染链路，断言输出不含 `http://`、典型 `host:port`、超长原文。

## Implementation Shape

### 1. Spec

在 `quality-guidelines.md` 的 Pool Quality Status And Metrics / Gateway 相关段落后，增加统一 **「Prometheus Low-Cardinality Contract」** 小节：

- 渲染入口表
- 全量 metric 清单
- 允许 label 表
- 禁止项
- fetcher/release 现状与未来约束
- Wrong/Correct 各一例

不新建 `metrics-cardinality.md`（D2）。

### 2. Tests in `status.rs`

在 `#[cfg(test)]` 增强：

| 测试 | 断言 |
|------|------|
| `metrics_label_allowlist_is_closed`（新） | 解析 `render_prometheus_metrics` 输出中所有 `{k="v"}`，每个 (metric,k,v) ∈ 白名单；无 label 的 metric 名集合也锁定 |
| `metrics_failure_reason_render_rejects_raw_error_substrings`（新） | 用含 URL/IP 的 reason 若被错误放入 `top_failure_reasons` 则不应发生——更稳妥：只测 `normalize_failure_reason` 全集 + 用合法 reason 渲染；另对**若**直接把 raw 塞进 `FailureReasonCount.reason` 的防御：当前类型是 `&'static str` 且只从 normalize 产出，故强化 normalize 全分支 + 渲染后扫描禁止模式 |
| 既有 `failure_reason_normalization_is_bounded` | 扩展覆盖全部归一化分支关键字 |
| 既有 `metrics_include_pool_dependency_warp_and_xray_values` | 保留 |

解析策略（轻量，无新依赖）：

- 按行扫描，跳过 `#` 行；
- 对含 `{` 的 sample 行用简单状态机或 regex-free 截取 `name{labels} value`；
- label 按 `key="value"` 逗号分隔。

### 3. Tests in `route_debug.rs`

| 测试 | 断言 |
|------|------|
| `gateway_metrics_emit_exactly_45_series`（新） | 计 `proxy_gateway_route_attempts_total{` 行数 == 45 |
| `gateway_metrics_label_allowlist_is_closed`（新） | protocol/exit/status 值均 ∈ 白名单 |
| 既有 `gateway_metrics_render_all_label_dimensions` | 保留 |

### 4. Optional code fix

仅当审计发现违规时最小修复（例如某 label 拼接了动态字符串）。预判：**当前实现合规**，以测试+文档为主。

### 5. README

若 `GET /api/metrics` 一行描述未提低基数，补半句；否则不动。

## Compatibility / Rollback

- 纯测试+文档时：回滚即还原文件，无运行时行为变化。
- 若有修复：保持 metric 名稳定；仅收紧 label 值（去掉高基数）属于可接受 breaking for scrapers that wrongly depended on raw labels（项目从未承诺 raw labels）。

## Trade-offs

| 选项 | 结论 |
|------|------|
| 抽共享 `assert_labels` helper | 不做（D4）；两处测试可各写小函数 |
| 独立 spec 文件 | 不做（D2） |
| 引入 prometheus crate | 不做 |

## Validation Commands

```bash
cargo fmt --all -- --check
cargo test -p proxy-core status::
cargo test -p proxy-core route_debug::
cargo test -p proxy-core
cargo clippy -p proxy-core --all-targets -- -D warnings
# 若未改 proxy-api 逻辑可跳过；若改拼接则：
cargo test -p proxy-api
```
