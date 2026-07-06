# PRD: CI/CD + MCP 自更新部署

## 目标

实现"Windows 本地改代码 → git push → GitHub Actions 自动构建镜像 → MCP 触发服务器拉取新镜像并重启"的完整闭环。

## 需求

### F1: GitHub Actions CI Workflow
- push 到 main 分支时自动触发
- 使用 cargo-chef 分层构建 Docker 镜像
- 推送到 GHCR: `ghcr.io/iamdreaming/proxy-pool-rust`
- 镜像打两个 tag: `latest` 和 `{git short sha}`

### F2: docker-compose.yml 改用 GHCR 镜像
- proxy-pool 服务从 GHCR 拉取镜像，不再本地构建
- 保留 local build 选项（注释形式）

### F3: MCP `update_service` 工具
- 容器内通过 Docker socket 预拉取 GHCR 镜像，再触发 Watchtower 更新当前容器
- 返回更新前后的镜像 ID / digest 对比
- 更新失败时旧容器继续运行
- 工具必须有显式安全开关，并且 Docker socket、容器名、镜像、Watchtower URL、token 都可配置，避免把生产更新能力硬编码进二进制

### F4: 版本信息接口
- `/api/status` 增加 `version` 和 `git_hash` 字段
- 编译时通过 `env!/option_env!` 宏注入 git hash

### F5: 启动稳定性
- 非核心后台任务（订阅刷新、MCP HTTP/stdio、xray 可选能力）失败时应记录错误并降级，不应让 API/Gateway 主服务无意义崩溃
- API 绑定失败属于核心错误，必须记录清晰日志并让主 select 感知任务停止

## 验收标准

1. `git push origin main` 后 GitHub Actions 成功构建并推送镜像到 GHCR
2. MCP `update_service` 工具能拉取新镜像并重启容器
3. 更新后 `/api/status` 显示新 git hash
4. 更新失败时旧容器继续运行
5. 未开启更新开关时，MCP `update_service` 返回结构化禁用状态，不接触 Docker socket
6. `cargo test --workspace --all-targets`、`cargo clippy --workspace --all-targets -- -D warnings`、`npm run build` 通过
