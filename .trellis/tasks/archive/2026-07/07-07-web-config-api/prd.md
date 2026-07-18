# 完善 Web 配置查看与编辑 API

## Goal

让 Web 的“系统设置”页面可以读取并编辑真实服务配置，替换当前“Web 配置查看与编辑 API 暂未接入”的占位状态。

用户价值：
- 运维人员可以在 Web 端查看当前 `config/settings.yaml` 对应的配置内容。
- 可通过 Web 提交配置修改，后端负责校验并安全写回配置文件。
- 保存结果会明确说明“需要重启后生效”，避免误以为运行中的 scheduler、gateway、MCP、xray 等组件已经热更新。

## Confirmed Facts

- `web/src/views/Settings.vue` 当前只展示空态，占位文案说明配置 API 未接入，配置仍以 `config/settings.yaml` 为准。
- `web/src/api/index.ts` 尚无 settings/config 相关 API wrapper。
- `crates/proxy-api/src/routes.rs` 当前没有 `/api/settings` 或类似配置读取/写入路由。
- `crates/proxy-server/src/main.rs` 启动时从 CLI 参数或默认 `config/settings.yaml` 加载配置，并把配置值注入 Redis、Scheduler、Validator、Gateway、MCP、Subscription、xray 等运行时组件。
- 当前代码未发现配置热加载机制；编辑配置文件后，至少部分运行时组件需要重启进程才能使用新配置。
- `crates/proxy-core/src/config.rs` 的 `Settings` 及子配置已实现 `Serialize` / `Deserialize`，可以作为读取、校验和序列化配置的基础。
- 配置内包含潜在敏感字段，至少包括 `redis.url` 和 `subscription.github.token`；订阅 URL 也可能携带 token、query 或 fragment。

## Decisions

- D1: 本任务第一版只写回 YAML 配置文件，不做全量运行时热加载；后端响应和前端页面都显示 `restart_required=true`。
- D2: API 默认返回脱敏后的配置展示值；保存时如果用户保留脱敏占位符，则后端沿用原文件中的敏感值，只有显式输入新值才覆盖。
- D3: 前端以结构化 JSON 编辑器形式展示配置对象，避免新增前端 YAML 解析依赖；后端仍写回 YAML 文件。

## Requirements

- R1: 后端提供 `GET /api/settings`，返回当前配置文件路径、脱敏后的配置对象、脱敏字段列表、重启生效标记。
- R2: 后端提供 `PUT /api/settings`，接受完整配置对象，先用现有 `Settings` 结构与额外业务规则校验，再写回配置文件。
- R3: 写回必须避免无效配置覆盖现有配置；校验失败时返回 400，并带可读错误信息。
- R4: 写回成功后，响应必须明确标记 `restart_required=true`。
- R5: 前端 Settings 页面接入真实 API，具备加载态、错误态、只读元信息、编辑、保存成功/失败反馈。
- R6: 前端不得继续展示“API 暂未接入”的占位状态。
- R7: 前端保存成功后重新加载最新配置，确保展示与文件内容一致。

## Acceptance Criteria

- [ ] `GET /api/settings` 返回当前配置内容、配置文件路径、脱敏字段列表和 `restart_required=true`。
- [ ] `PUT /api/settings` 可以校验并写回有效配置。
- [ ] 提交无效配置时返回 400，原配置文件不被覆盖。
- [ ] 保留脱敏占位符保存时，原敏感字段不会被占位符覆盖。
- [ ] 保存成功响应和 Web 页面都明确提示需要重启后生效。
- [ ] Settings 页面能从后端加载真实配置，保存成功后刷新展示最新配置。
- [ ] 后端测试覆盖读取/写入辅助逻辑、写入失败不覆盖、敏感占位符保留。
- [ ] 前端至少通过 type-check/build 验证。

## Out of Scope

- 本任务不实现全量运行时热加载。
- 本任务不修改生产配置或触发远端重启。
- 本任务不接入权限系统；若后续项目引入鉴权，需要单独加保护。
- 路由规则 `config/routes.example.yaml` / `routes_path` 的独立规则编辑 API 暂不包含在本任务。
- 保留原 YAML 注释与排版不是本任务验收项；后端会按 `Settings` 结构写回规范化 YAML。
