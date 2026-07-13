# fix-degraded-pool-and-xray-failures

## Goal

修复代理池 degraded 状态：Xray 节点全失败、验证成功率低、失败原因被吞、驱逐策略过松、Gateway 缺少 HTTP 正向代理。

## Requirements

### P0: Xray 验证超时修复
- 将 xray `validate_timeout_sec` 默认值从 5 提升到 15（与池代理一致）
- 降低 `failure_cooldown_secs` 默认值让失败节点更快重试（3600 → 600）

### P1: 传播真实失败原因
- `mark_failed` / `mark_failed_with_circuit` 应接收并存储具体的错误类型标签（Timeout/RequestFailed/BadStatus/BodyReadFailed/ClientBuildFailed/InvalidProxyUrl）而非硬编码 `"validation_failed"`
- `normalize_failure_reason` 应保留原始错误类型标签
- `/api/status` 的 `top_failure_reasons` 应展示真实失败分类

### P2: 收紧驱逐策略
- 将 `min_score` 默认值从 0.1 提升到 0.3
- 降低 `hard_failure_evict` 阈值：从 `fail_count > max(8, success_count * 3)` 改为 `fail_count > max(5, success_count * 2)`

### P3: 增加按源存活率门控
- 在 scheduler 中利用已有的 `validation_survival_rate`（`fetcher/base.rs:329-331`）对低存活率源自动降频或暂停抓取
- 存活率 < 10% 的源暂停 1 小时

### P4: Gateway HTTP 正向代理
- 在 `http_connect.rs` 中增加对非 CONNECT 请求的处理：解析绝对 URI，通过上游代理转发请求/响应
- 保持现有 CONNECT 隧道逻辑不变

## Acceptance Criteria

- [ ] `cargo test` 零失败
- [ ] `cargo clippy -- -D warnings` 零警告
- [ ] Xray 节点验证超时 ≥15s，failure cooldown ≤600s
- [ ] `/api/status` 的 `top_failure_reasons` 展示具体错误类型而非统一 `validation_failed`
- [ ] `min_score` 默认值 ≥0.3
- [ ] `hard_failure_evict` 阈值降低
- [ ] 低存活率源自动暂停
- [ ] Gateway 支持普通 HTTP 代理请求（`curl -x http://gateway http://target` 返回 200 而非 400）

## Notes

- 配置默认值修改需同步更新 `settings.example.yaml`
- P0 是配置变更，P1-P3 是代码变更，P4 是新功能
