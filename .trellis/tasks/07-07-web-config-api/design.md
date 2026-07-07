# Design: Web 配置查看与编辑 API

## Backend Boundary

新增配置 API 放在 `crates/proxy-api/src/routes.rs`，并通过 `AppState` 持有当前配置文件路径：

- `GET /api/settings`
  - 读取 `state.config_path` 指向的 YAML。
  - 使用 `proxy_core::config::Settings` 解析；文件不存在时沿用 `Settings::default()` 的现有行为。
  - 返回脱敏后的 `settings`、`path`、`redacted_fields`、`restart_required=true`、`status="ok"`。

- `PUT /api/settings`
  - 接受 `{ "settings": Settings }`。
  - 将脱敏占位符与当前文件中的真实敏感值合并。
  - 运行额外校验，例如空 URL、xray 端口范围倒置、负权重等。
  - 序列化为 YAML 并写回 `state.config_path`。
  - 返回与 GET 同形的最新脱敏配置，并保持 `restart_required=true`。

## Sensitive Values

使用固定占位符：

`__PROXY_POOL_REDACTED__`

第一版处理字段：

- `redis.url`
- `subscription.github.token`

保存规则：

- 如果提交值仍等于占位符，后端使用原配置文件中的真实值。
- 如果提交值是新字符串，后端写入新值。
- 如果 `subscription.github.token` 提交为 `null`，后端清空 token。

订阅 URL 中 query/fragment 可能也包含秘密，但这些字段数量与索引会变化。第一版不做自动字段级替换；后续可扩展为 URL 局部脱敏与路径级 merge。

## Write Safety

写入前先完成 JSON 反序列化、敏感值合并、业务校验和 YAML 序列化。无效输入不会触碰原文件。

写文件时使用临时文件生成新 YAML，验证临时内容可重新解析为 `Settings` 后再替换目标文件；Windows 下若不能原子覆盖，则采用受控替换并在失败时尽量恢复原内容。

## Frontend Boundary

`web/src/api/index.ts` 新增 settings API wrapper，`web/src/types/index.ts` 新增配置响应类型。

`web/src/views/Settings.vue` 改成真实配置编辑页：

- 加载中显示表格/编辑器 loading。
- 顶部展示配置路径、脱敏字段数量、重启生效状态。
- 主要编辑区域使用 JSON textarea，内容来自后端 `settings`。
- 保存前在前端做 JSON.parse；解析失败直接展示错误，不发请求。
- 保存成功后提示并重新 GET 刷新。

## Compatibility

- 不改变现有启动参数；`crates/proxy-server/src/main.rs` 继续用 CLI 参数决定配置路径，并把同一路径传入 `AppState`。
- 不改变运行时组件配置注入方式；保存后仍需要用户重启进程。
- `serde_yaml` 写回会规范化 YAML，可能移除原文件注释和手工排版。

## Risks

- 当前 Web/API 没有鉴权，配置写入接口默认只适合受信任网络。部署层应继续限制访问面。
- 脱敏字段第一版只覆盖明确结构化秘密；订阅 URL 内嵌秘密暂不自动脱敏。
- 写回 YAML 后如果用户依赖注释，需要从版本控制或示例文件查看说明。
