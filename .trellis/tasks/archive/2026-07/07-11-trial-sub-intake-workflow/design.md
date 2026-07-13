# Design: 试用订阅人工接入工作流

## Boundaries

- **纯文档任务**：不新增代码，不新增 API/MCP 端点
- **利用现有机制**：`subscription.urls` 配置 + `refresh_subscription_source` MCP/REST + `xray_status` + `service_status`
- **安全红线**：文档中明确禁止自动注册，代码库中零自动注册模块

## F1 — Operator intake path

在 `docs/trial-sub-intake.md` 中记录：

1. 配置方式：`config/settings.yaml` 中 `subscription.urls` 添加 URL
2. Preview 流程：`refresh_subscription_source(source="id", apply=false)`
3. Apply 流程：`refresh_subscription_source(source="id", apply=true)`
4. 批量 URL：YAML 列表 + 逐个 preview

## F2 — Quality gate after apply

在文档中记录 apply 后的检查步骤：

1. `subscription_sources` — 查看 parse/activation 报告
2. `xray_status` — 查看 active/failed 节点数
3. `service_status` — 查看 `pool.tier` 是否达到 `stable`
4. `explain_proxy_scores` — 抽样检查海外节点质量
5. 禁用坏源：YAML 中 `enabled: false` 或移除 URL

## F3 — Safety

文档中明确声明：

- 禁止自动注册、验证码绕过、批量账号工厂
- 代码库中无此类模块
- `reject` recommendation 阻止默认 apply

## F4 — UX

- 优先文档 + 现有 API/MCP
- 不新增 Dashboard
- 批量 URL 通过 YAML 列表实现，无需额外工具

## Compatibility

- 无代码变更，无兼容性影响
