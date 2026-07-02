# Phase 3 PRD: 补全闭环

## 背景

Phase 1 完成了核心模型、存储、验证、调度和订阅解析。Phase 2 完成了 xray-core 集成、网关路由、熔断器和进程监管。但多个关键路径仍存在 stub/TODO，系统无法端到端可靠运行。

Phase 3 目标：**让现有功能真正可用、可靠**，不引入新架构，只补全缺口。

---

## 需求列表

### F1: API stub 端点补全

**现状**：`proxy-api` 的 `refresh_pool` 返回假数据，`delete_proxy` 返回 501。

**需求**：
- `POST /api/proxies/refresh` 触发 Scheduler 执行一次 fetch+validate+store 循环，返回实际执行状态
- `DELETE /api/proxy/{key}` 从 ProxyStore 中移除指定代理，返回操作结果
- 需要跨 crate 通信机制（API → Scheduler/Store）

**验收标准**：
- [ ] `POST /api/proxies/refresh` 实际触发 scheduler.run_once()，返回 `{"status":"ok","fetched":N,"validated":M}`
- [ ] `DELETE /api/proxy/{key}` 成功移除代理，返回 `{"status":"ok"}`；key 不存在返回 404
- [ ] 两个端点有对应的集成测试

### F2: MCP stub 工具补全

**现状**：`proxy-mcp` 的 `geoip_lookup` 返回占位文本，`refresh_pool` 返回占位文本。

**需求**：
- `geoip_lookup` 调用 `GeoIPLookup` 实例，返回国家/城市/ASN 信息
- `refresh_pool` 触发 Scheduler 执行一次刷新，返回实际结果
- MCP server 需要持有 `GeoIPLookup` 和 Scheduler channel 的引用

**验收标准**：
- [ ] `geoip_lookup` 返回结构化 GeoIP 结果（country, city, asn, is_domestic）
- [ ] `refresh_pool` 实际触发刷新并返回结果
- [ ] 两个工具有单元测试

### F3: WarpChain 链式代理实现

**现状**：`proxy-gateway` 的 `Upstream::WarpChain` 变体返回 501/不支持。

**需求**：
- 实现 WarpChain 代理：先通过普通代理（SOCKS5）连接到 WARP 入口，再通过 WARP 出口访问目标
- HTTP CONNECT 和 SOCKS5 两种协议都需支持
- 链式连接：client → gateway → proxy(SOCKS5) → WARP(SOCKS5) → target

**验收标准**：
- [ ] HTTP CONNECT 请求匹配 WarpChain 路由时，成功建立链式隧道
- [ ] SOCKS5 CONNECT 请求匹配 WarpChain 路由时，成功建立链式隧道
- [ ] 链式连接失败时返回适当错误（502 for HTTP, general failure for SOCKS5）
- [ ] 有单元测试覆盖链式连接逻辑

### F4: xray gRPC 自动重连

**现状**：`XrayClient::connect()` 仅在启动时调用一次，断连后无法恢复。

**需求**：
- gRPC 连接断开后自动重连（指数退避，最大间隔 30s）
- 重连成功后恢复 add/remove 操作
- outbound_sync 循环中检测连接状态，断连时暂停同步

**验收标准**：
- [ ] gRPC 断连后 30s 内自动重连
- [ ] 重连期间 add/remove 操作返回明确错误（非 panic）
- [ ] outbound_sync 在断连期间暂停，重连后自动恢复
- [ ] 有测试覆盖重连逻辑

### F5: 测试覆盖补全

**现状**：proxy-api、proxy-mcp、proxy-gateway 协议处理器零测试。

**需求**：
- proxy-api：每个端点的 happy-path 测试
- proxy-mcp：每个工具的 happy-path 测试
- proxy-gateway：HTTP CONNECT / SOCKS5 处理逻辑的单元测试（mock upstream）
- proxy-core：store 和 validator 的 mock 测试

**验收标准**：
- [ ] `cargo test` 全部通过
- [ ] proxy-api 端点测试覆盖 ≥6 个（每个路由至少 1 个）
- [ ] proxy-mcp 工具测试覆盖 ≥8 个（每个工具至少 1 个）
- [ ] proxy-gateway 新增 ≥3 个协议处理测试
- [ ] proxy-core 新增 store/validator mock 测试

---

## 非目标（Phase 3 不做）

- Web UI / CLI 管理工具
- 本地持久化 fallback（SQLite 等）
- 流量统计/计费/多用户认证
- 新的代理获取源
- 性能优化（连接池、零拷贝等）

---

## 约束

- 不引入新 crate 依赖（除测试所需的 mock 库）
- 不改变现有公共 API 签名（可扩展，不破坏）
- 不改变 Redis 存储模型
- 所有修改须 `cargo clippy -- -D warnings` 零警告
- 所有修改须 `cargo test` 零失败

---

## 依赖关系

```
F1 (API stub) ──┐
                ├── 需要 Scheduler channel 机制
F2 (MCP stub) ──┘

F3 (WarpChain) ── 独立

F4 (gRPC 重连) ── 独立

F5 (测试) ── 依赖 F1-F4 完成（测试补全的代码）
```

建议实施顺序：F4 → F3 → F1+F2（并行）→ F5

---

## 风险

| 风险 | 影响 | 缓解 |
|------|------|------|
| Scheduler channel 设计不当导致死锁 | 高 | 使用 mpsc::channel + try_send，非阻塞 |
| WarpChain 链式连接延迟叠加 | 中 | 设置每跳超时，总超时 = 各跳超时之和 |
| gRPC 重连与 outbound_sync 竞态 | 中 | 用 tokio::watch 传递连接状态，sync 侧 watch 变化 |
| mock 测试引入新依赖 | 低 | 仅在 [dev-dependencies] 添加 mockall/tokio-test |
