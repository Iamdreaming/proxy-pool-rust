# Implement: 试用订阅人工接入工作流

## Slice 1 — docs/trial-sub-intake.md

**文件**：`docs/trial-sub-intake.md`（新建）

内容：
1. 端到端流程：paste URL → config → preview → apply → observe
2. Quality gate 检查步骤
3. 安全红线声明
4. 失败/禁用路径
5. MCP/REST 示例

## Slice 2 — 最终验证

1. `cargo test --workspace`（无代码变更，确认无回归）
2. `cargo clippy --workspace -- -D warnings`
