# proxy-pool-rust

> Rust 重写的自建代理池系统：免费代理抓取 → 可用性验证 → 入池管理 → 智能路由

## 功能特性

- 🔄 **自动抓取**：从 5+ 个免费代理源自动抓取 HTTP/HTTPS/SOCKS5 代理
- ✅ **可用性验证**：并发验证连通性、延迟、匿名度（透明/匿名/高匿）
- 📊 **评分系统**：基于延迟、成功率、匿名度的加权评分自动排序
- 🔁 **智能路由**：域名规则 + GeoIP 自动分流 + 智能回退链
- 🌐 **代理网关**：纯 Rust 实现 HTTP CONNECT + SOCKS5 代理网关
- 🔗 **链式代理**：WarpChain 池代理 → WARP → 目标，双重跳转出口
- 🛡️ **Circuit Breaker**：三态熔断器自动剔除不可用代理
- 🔐 **xray 集成**：SS/VMess/Trojan 加密节点自动激活，gRPC 自动重连
- 📡 **订阅管理**：GitHub 自动发现 + URL 聚合 + Base64/YAML/Clash 解析
- 🔌 **MCP Server**：内置 MCP 服务，供 LLM 直接调试代理池
- ☁️ **WARP 集成**：Cloudflare WARP 端点优选 + 健康检查 + 负载均衡
- 🌍 **GeoIP 识别**：MaxMind GeoLite2 境内外自动识别
- 📋 **Web Dashboard**：Vue3 + Naive UI 管理面板（开发中）

## 项目结构

```
proxy-pool-rust/
├── crates/
│   ├── proxy-core/       # 核心库（模型、存储、验证、调度、GeoIP、路由、WARP）
│   ├── proxy-api/        # REST API 服务 (axum)
│   ├── proxy-gateway/    # HTTP CONNECT + SOCKS5 代理网关
│   ├── proxy-mcp/        # MCP Server (rmcp)
│   ├── proxy-sub/        # 订阅管理（发现、解析、待入池）
│   ├── proxy-xray/       # xray-core gRPC 集成（进程管理、出站同步、重连）
│   └── proxy-server/     # 主入口，组合所有服务
├── web/                  # Vue3 前端 (开发中)
├── config/               # YAML 配置文件
└── deploy/               # Docker 部署
```

## 快速开始

### 前置条件

- Rust 1.85+ (edition 2024)
- Redis 6+
- (可选) xray-core — 加密节点代理
- (可选) Docker + Docker Compose — 容器化部署

### 编译运行

```bash
# 编译
cargo build --release

# 复制配置文件
cp config/settings.example.yaml config/settings.yaml

# 运行
./target/release/proxy-server config/settings.yaml
```

### Docker 部署

使用 cargo-chef 分层构建，依赖变更时才重编译依赖层，业务代码变更重建约 3-5 分钟：

```bash
cd deploy
cp ../config/settings.example.yaml ../config/settings.yaml
# 编辑 settings.yaml（至少修改 redis.url）
# 可选：设置 Watchtower 更新 token；不设置时使用 compose 中的默认本地 token
# export PROXY_POOL_UPDATE_TOKEN="your-token"
docker compose up -d
```

查看日志：

```bash
docker compose logs -f proxy-pool
```

## 路由决策链

网关收到请求后按以下顺序选择出口：

```
1. 路由规则匹配 → direct / free_pool / warp / xray
2. GeoIP 自动分流 → 境内直连，境外走代理
3. 回退链 → 池代理 → WARP → xray → 502
```

## API 端点

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/status` | 系统状态、版本、git hash、release 镜像配置、协议分布和只读质量摘要 |
| GET | `/api/readyz` | 依赖 readiness |
| GET | `/api/routes/test` | 路由 dry-run 诊断 |
| GET | `/api/fetchers` | 抓取源状态和源级熔断状态 |
| POST | `/api/fetchers/{id}/refresh` | 手动刷新/探测单个抓取源 |
| GET | `/api/subscriptions/sources` | 订阅源状态、最近刷新报告和解析统计 |
| POST | `/api/subscriptions/sources/{id}/refresh` | 手动 preview/apply 单个订阅源刷新，默认 preview |
| GET | `/api/proxies` | 代理列表（支持 protocol/limit 参数） |
| GET | `/api/proxies/scores` | 代理评分解释 |
| POST | `/api/proxy/check-matrix` | 对单个代理执行多目标验证矩阵，返回每个目标的状态、耗时和出口信息 |
| GET | `/api/proxy/random` | 随机代理 |
| GET | `/api/proxy/best` | 最佳代理 |
| POST | `/api/proxies/refresh` | 触发刷新（返回抓取/验证/存储计数） |
| DELETE | `/api/proxy/{key}` | 删除代理（key 格式：protocol:host:port） |
| GET | `/api/metrics` | Prometheus 指标，包含代理池规模、只读质量摘要、依赖和路由指标 |
| GET | `/api/xray/status` | xray 节点生命周期、活跃/失败计数和最近状态 |

## MCP Server

内置 MCP Server 支持 stdio 和 Streamable HTTP 两种传输方式，供 LLM 直接调试代理池：

### MCP Tools

| Tool | 说明 |
|------|------|
| `get_proxy` | 获取一个可用代理 |
| `get_best_proxy` | 获取评分最高的代理 |
| `list_proxies` | 列出池中代理（支持 protocol/limit 参数） |
| `explain_proxy_scores` | 查看代理评分组成和保留决策 |
| `cleanup_low_score_proxies` | dry-run 或显式 apply 清理低分代理 |
| `check_proxy` | 验证指定代理可用性，并返回目标、耗时、HTTP 状态和出口信息 |
| `check_proxy_matrix` | 对单个代理执行多目标验证矩阵，默认检查 Cloudflare trace 和 httpbin IP |
| `service_status` | 查看结构化服务状态、release metadata、依赖、代理池摘要和只读质量摘要 |
| `update_status` | 查看最近一次 `update_service` 结果，不触发更新 |
| `update_service` | 拉取配置镜像并触发 Watchtower 更新，需显式启用 `PROXY_POOL_UPDATE_ENABLED=true` |
| `pool_status` | 查看池状态概览 |
| `warp_status` | WARP 实例状态 |
| `xray_status` | xray 节点生命周期、活跃/失败计数和最近状态 |
| `refresh_pool` | 触发抓取+验证（返回实际结果） |
| `fetcher_status` | 查看抓取源状态和源级熔断状态 |
| `refresh_fetcher` | 手动刷新/探测单个抓取源 |
| `subscription_sources` | 查看订阅源状态、最近刷新报告和解析统计 |
| `refresh_subscription_source` | 手动 preview/apply 单个订阅源刷新，默认 preview |
| `route_test` | 路由 dry-run 诊断 |
| `remove_proxy` | 移除代理 |
| `proxy_stats` | 代理池统计（协议分布） |
| `geoip_lookup` | 查询地理位置（国家/境内外） |

## Dev 验证

推送后的 dev 验证默认走 GitHub Actions、公开 HTTP 状态接口和 MCP 只读
`service_status` / `update_status`。不要直接 SSH 到 dev 地址；`update_service`
只在明确选择更新时调用，不作为默认状态检查。完整清单见
[`docs/dev-validation.md`](docs/dev-validation.md)。

常用的一条命令式只读检查：

```bash
python tests/integration/readonly_dev_smoke.py --branch main --wait-ci
```

它只读取 GitHub Actions、`/api/status`、`/api/readyz`、MCP `service_status`
和 `update_status`，不会触发远端更新。

### 配置示例 (Claude Desktop / ZCode)

```json
{
  "mcpServers": {
    "proxy-pool": {
      "command": "proxy-server",
      "args": ["config/settings.yaml"],
      "env": {
        "MCP_TRANSPORT": "stdio"
      }
    }
  }
}
```

## xray 加密代理集成

启用 xray 集成后，系统自动管理 SS/VMess/Trojan 加密节点：

1. 订阅源拉取加密节点 → 写入 Redis 待入池队列
2. OutboundSync 循环读取待入池节点 → 分配本地 SOCKS5 端口 → 配置 xray-core
3. gRPC 连接断开自动重连（指数退避 1s→30s），同步循环自动暂停/恢复

配置示例：

```yaml
xray:
  enabled: true
  binary_path: "xray"          # xray-core 二进制路径
  api_port: 10085              # gRPC API 端口
  port_range_start: 20000      # 本地 SOCKS5 端口范围
  port_range_end: 29999
  sync_interval_sec: 30        # 同步间隔
  max_active_nodes: 5000       # 最大活跃节点数
```

## 代理源

内置支持以下免费代理源：

| 源 | 协议 | 类型 |
|----|------|------|
| ProxyScrape | HTTP/SOCKS5 | API |
| TheSpeedX | HTTP/SOCKS5 | GitHub 列表 |
| Free Proxy List | HTTP/HTTPS | HTML 表格 |
| Clarketm | HTTP | GitHub 列表 |
| GeoNode | HTTP/SOCKS | API |

## 配置

详见 `config/settings.example.yaml`，所有字段都有默认值，只需覆盖需要修改的部分。

## 许可证

MIT License
