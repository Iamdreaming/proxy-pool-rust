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
- 测试：每个 crate 的 `tests/` 目录，`cargo test` 运行
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
