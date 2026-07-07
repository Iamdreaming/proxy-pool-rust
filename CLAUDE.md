# proxy-pool-rust 项目指令

## 项目概述
Rust workspace 代理池服务，包含代理获取、验证、网关路由、MCP 接口和 WARP 链式代理。

## Workspace 结构
- `crates/proxy-core` — 核心模型、存储、验证、GeoIP、WARP 模型
- `crates/proxy-api` — REST API (axum)
- `crates/proxy-gateway` — HTTP 代理网关（路由、链式代理、上游选择）
- `crates/proxy-mcp` — MCP Server (rmcp)
- `crates/proxy-server` — 主入口，组装各 crate

## 编码规范
- Edition 2024，Rust latest stable
- 错误处理：库代码用 `thiserror`，应用代码用 `anyhow`
- 异步：`tokio` runtime，`async-trait` 用于 trait 异步方法
- 日志：`tracing` + `tracing-subscriber`，不用 `log` crate
- 序列化：`serde` + `serde_json`/`serde_yaml`
- 测试：每个 crate 的 `tests/` 目录，`cargo test` 运行；新增功能须覆盖 happy-path + 关键 error-path；`cargo test` 零失败方可提交
- Lint：`cargo clippy -- -D warnings`

## 常用命令
- 构建：`cargo build`
- 测试：`cargo test`
- Lint：`cargo clippy -- -D warnings`
- 格式化：`cargo fmt`
- 运行：`cargo run -p proxy-server`

## fast-context 搜索参数
- 本项目 5 个 crate，中等规模：`tree_depth=2, max_turns=2`
- 跨 crate 调用链追踪：`max_turns=3`

## 修复后部署与端到端验证工作流

禁止直接 SSH 到 dev 地址。部署后默认验证必须走 GitHub Actions、HTTP 状态
接口和 MCP 只读状态工具；`update_service` 是显式选择的变更动作，不是默认
状态检查。完整检查清单见 `docs/dev-validation.md`。

1. 修复/实现代码后，先在本地运行 `cargo test` 和 `cargo clippy -- -D warnings`，零失败零警告方可进入下一步。
2. 提交并用 conventional commits 格式（如 `fix(core): ...` / `feat(api): ...`，破坏性变更加 `!`）。
3. `git push origin main` 推送到远程 main 分支。
4. 用 `gh` CLI 监视 GitHub Actions 构建状态，判定镜像构建完成：
   - `gh run list --workflow=docker-build.yml --branch main --limit 1` 找到刚触发的 run。
   - `gh run watch <run-id> --exit-status` 阻塞等待直到 run 完成；非零退出码即构建失败，停止后续步骤并向用户报告失败日志（`gh run view <run-id> --log-failed`）。
5. 构建成功后，先走只读验证：访问 `/api/status` / `/api/readyz`，并调用 MCP `service_status` / `update_status` 查看当前 runtime git hash、release metadata 和最近更新状态。
6. 如果只读状态证明 dev 仍是旧镜像，且用户或 operator 明确选择更新，再调用 MCP 工具 `mcp__proxy-pool__update_service` 拉取 GHCR 新镜像并触发 Watchtower；随后通过 HTTP/MCP 只读状态确认 `git_hash` 已更新为本次推送的 short sha。
7. 端到端验证：调用 `mcp__proxy-pool__pool_status` 确认服务存活；按需抽样 `mcp__proxy-pool__get_best_proxy` / `mcp__proxy-pool__check_proxy` 验证代理池可用性。
8. 任意一步失败都不得继续后续步骤；失败时定位失败点、修复后从第 1 步重新开始。
