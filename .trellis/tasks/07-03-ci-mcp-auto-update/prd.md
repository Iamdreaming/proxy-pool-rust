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
- 容器内通过 Docker socket 执行 `docker pull` + `docker compose up -d`
- 返回更新前后的镜像 digest 对比
- 更新失败时旧容器继续运行

### F4: 版本信息接口
- `/api/status` 增加 `version` 和 `git_hash` 字段
- 编译时通过 `env!/option_env!` 宏注入 git hash

## 验收标准

1. `git push origin main` 后 GitHub Actions 成功构建并推送镜像到 GHCR
2. MCP `update_service` 工具能拉取新镜像并重启容器
3. 更新后 `/api/status` 显示新 git hash
4. 更新失败时旧容器继续运行
