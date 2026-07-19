# Implement: metrics-low-cardinality-audit-v1

## Ordered Checklist

1. **Audit freeze**  
   再扫一遍 `render_prometheus_metrics` 与 `GatewayRouteMetrics::render_prometheus`，确认无第三渲染源、无动态 label 拼接。将结果记入 implement.jsonl 对应 research 笔记（可选短文 `research/metrics-inventory.md`）。

2. **Tests — status**（`crates/proxy-core/src/status.rs`）  
   - 扩展 `failure_reason_normalization_is_bounded` 覆盖全部关键字分支。  
   - 新增 allowlist 闭合测试（解析 exposition label）。  
   - 新增：合成 quality 渲染后，metrics 文本不得匹配高基数模式（如 `http://`、明显 `digits.digits.digits.digits:port` 作为 label 值——注意 HELP 文本不含这些即可；重点扫 `{...}` 内）。

3. **Tests — gateway**（`crates/proxy-core/src/route_debug.rs`）  
   - 45 series 计数。  
   - label allowlist 闭合。

4. **Fix if red**  
   仅当测试暴露违规时最小改渲染/归一化。

5. **Spec**  
   更新 `quality-guidelines.md`：Prometheus Low-Cardinality Contract 全量清单 + 禁止规则 + fetcher/release 备注。必要时改 `index.md` 一行描述。

6. **README**  
   仅当 metrics 行描述不足时补低基数说明。

7. **ROADMAP**  
   start 后把该项移入 Now；完成时勾选候选功能并迁 Done（finish 阶段）。

8. **Validate**  
   跑 design 中的 cargo 命令；全绿再进入 check / commit。

## Risky Files

| File | Risk |
|------|------|
| `crates/proxy-core/src/status.rs` | 渲染格式变更可能影响 scraper；优先只加测试 |
| `crates/proxy-core/src/route_debug.rs` | 同上 |
| `.trellis/spec/proxy-core/backend/quality-guidelines.md` | 文档漂移 |

## Rollback Points

- 仅测试失败：修测试或修违规 label。  
- 误改 metric 名：立即恢复名称，只改 label 值策略。

## Review Gates Before `task.py start`

- [x] `prd.md` 收敛（D1–D4，无 open questions）
- [x] `design.md` / `implement.md` 齐备
- [ ] 用户审阅规划并批准 start
- [ ] `implement.jsonl` / `check.jsonl` 填入真实 spec 条目（sub-agent 模式）

## Validation Commands

```bash
cargo fmt --all -- --check
cargo test -p proxy-core
cargo clippy -p proxy-core --all-targets -- -D warnings
```
