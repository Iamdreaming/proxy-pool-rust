# proxy-pool-rust

> Rust 重写的自建代理池系统：免费代理抓取 → 可用性验证 → 入池管理 → 智能路由

## 功能特性

- 🔄 **自动抓取**：从 5+ 个免费代理源自动抓取 HTTP/HTTPS/SOCKS5 代理
- ✅ **可用性验证**：并发验证连通性、延迟、匿名度（透明/匿名/高匿）
- 📊 **评分系统**：基于延迟、成功率、匿名度的加权评分自动排序
- 🔁 **智能路由**：域名规则 + GeoIP 自动分流 + 智能回退链
- 🌐 **代理网关**：纯 Rust 实现 HTTP CONNECT + SOCKS5 代理网关
- 🛡️ **Circuit Breaker**：三态熔断器自动剔除不可用代理
- 🔌 **MCP Server**：内置 MCP 服务，供 LLM 直接调试代理池
- ☁️ **WARP 集成**：Cloudflare WARP 端点优选 + 健康检查 + 负载均衡
- 📋 **Web Dashboard**：Vue3 + Naive UI 管理面板（开发中）
- 🌍 **GeoIP 识别**：MaxMind GeoLite2 境内外自动识别

## 项目结构

```
proxy-pool-rust/
├── crates/
│   ├── proxy-core/       # 核心库（模型、存储、验证、调度、GeoIP、路由）
│   ├── proxy-api/        # REST API 服务 (axum)
│   ├── proxy-gateway/    # HTTP CONNECT + SOCKS5 代理网关
│   ├── proxy-mcp/        # MCP Server (rmcp)
│   └── proxy-server/     # 主入口，组合所有服务
├── web/                  # Vue3 前端 (开发中)
├── config/               # YAML 配置文件
└── deploy/               # Docker 部署
```

## 快速开始

### 前置条件

- Rust 1.85+ (edition 2024)
- Redis 6+
- (可选) Docker + Docker Compose (用于 WARP 容器管理)

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

```bash
docker compose up -d
```

## MCP Server

内置 MCP Server 支持 stdio 和 Streamable HTTP 两种传输方式，供 LLM 直接调试代理池：

### MCP Tools

| Tool | 说明 |
|------|------|
| `get_proxy` | 获取一个可用代理 |
| `get_best_proxy` | 获取评分最高的代理 |
| `list_proxies` | 列出池中代理 |
| `check_proxy` | 验证指定代理可用性 |
| `pool_status` | 查看池状态概览 |
| `warp_status` | WARP 实例状态 |
| `refresh_pool` | 触发抓取+验证 |
| `remove_proxy` | 移除代理 |
| `proxy_stats` | 代理池统计 |
| `geoip_lookup` | 查询地理位置 |

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

## API 端点

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/status` | 系统状态 |
| GET | `/api/proxies` | 代理列表 |
| GET | `/api/proxy/random` | 随机代理 |
| GET | `/api/proxy/best` | 最佳代理 |
| POST | `/api/proxies/refresh` | 触发刷新 |
| GET | `/api/metrics` | Prometheus 指标 |

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
