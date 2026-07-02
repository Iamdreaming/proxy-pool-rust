# Phase 3 Implement: 补全闭环

## 实施顺序

```
F4 (gRPC 重连) → F3 (WarpChain) → F1 (API stub) + F2 (MCP stub) → F5 (测试)
```

理由：F4 和 F3 独立且不依赖其他 feature；F1/F2 共享 Scheduler channel 机制，可并行；F5 依赖所有功能完成。

---

## Checklist

### F4: xray gRPC 自动重连

- [ ] 4.1 `proxy-xray/src/xray_client.rs`：新增 `connected: watch::Sender<bool>` 和 `connected_rx: watch::Receiver<bool>` 字段
- [ ] 4.2 `XrayClient::new()` 初始化 watch channel（初始值 false）
- [ ] 4.3 `XrayClient::connect()` 成功后 `connected.send(true)`
- [ ] 4.4 新增 `XrayClient::reconnect_loop()` — 后台重连循环，指数退避（1s→2s→...→30s max）
- [ ] 4.5 新增 `XrayClient::health_check()` — 简单 gRPC 调用检测连接存活
- [ ] 4.6 `XrayClient::ensure_connected()` — 检查连接状态，断连时返回错误
- [ ] 4.7 `add_inbound`/`add_outbound`/`remove_inbound`/`remove_outbound` 调用前检查连接状态
- [ ] 4.8 `proxy-xray/src/outbound_sync.rs`：`sync_loop` 接收 `watch::Receiver<bool>`，断连时暂停同步，重连后立即同步
- [ ] 4.9 `proxy-server/src/main.rs`：spawn `xray_client.reconnect_loop()`，传递 `connected_rx` 给 outbound_sync
- [ ] 4.10 `cargo clippy -- -D warnings` 零警告
- [ ] 4.11 `cargo test` 零失败

**验证命令**：`cargo build -p proxy-xray && cargo test -p proxy-xray`

---

### F3: WarpChain 链式代理

- [ ] 3.1 `proxy-gateway/src/upstream.rs`：新增 `connect_via_warp_chain(proxy, warp_port, target) -> Result<TcpStream>`
- [ ] 3.2 Step 1：复用 `connect_via_socks5` 连接到 WARP SOCKS5 入口
- [ ] 3.3 Step 2：在返回的 stream 上执行 SOCKS5 handshake 到 target
- [ ] 3.4 提取 `socks5_handshake(stream, target)` 为公共函数（从 `socks5.rs` 中提取握手逻辑）
- [ ] 3.5 `proxy-gateway/src/http_connect.rs`：WarpChain 分支调用 `connect_via_warp_chain`，成功后双向转发
- [ ] 3.6 `proxy-gateway/src/socks5.rs`：WarpChain 分支调用 `connect_via_warp_chain`，成功后双向转发
- [ ] 3.7 链式连接超时：每跳 10s，总超时 20s
- [ ] 3.8 错误处理：链式连接失败返回 502（HTTP）/ general failure（SOCKS5）
- [ ] 3.9 `cargo clippy -- -D warnings` 零警告
- [ ] 3.10 `cargo test` 零失败

**验证命令**：`cargo build -p proxy-gateway && cargo test -p proxy-gateway`

---

### F1: API stub 端点补全

- [ ] 1.1 `proxy-core/src/scheduler.rs`：定义 `SchedulerCommand` enum 和 `SchedulerResult` struct
- [ ] 1.2 `proxy-core/src/scheduler.rs`：定义 `SchedulerHandle`（持有 `mpsc::Sender<SchedulerCommand>`）
- [ ] 1.3 `SchedulerHandle::refresh()` — 发送 Refresh 命令，等待 oneshot 结果
- [ ] 1.4 `Scheduler` 新增 `cmd_rx: Option<mpsc::Receiver<SchedulerCommand>>` 字段
- [ ] 1.5 `Scheduler::run()` 中用 `tokio::select!` 同时监听 interval 和 cmd_rx
- [ ] 1.6 收到 Refresh 命令时调用 `run_once()`，通过 oneshot 返回结果
- [ ] 1.7 `Scheduler::new()` 接受可选 `cmd_rx` 参数
- [ ] 1.8 `proxy-core/src/store.rs`：新增 `ProxyStore::remove(&self, proxy: &Proxy) -> Result<bool>`
- [ ] 1.9 `proxy-api/src/routes.rs`：`AppState` 新增 `scheduler_handle: SchedulerHandle` 字段
- [ ] 1.10 `POST /api/proxies/refresh` — 调用 `scheduler_handle.refresh()`，返回 `{"status":"ok","fetched":N,"validated":M,"stored":S,"errors":E}`
- [ ] 1.11 `DELETE /api/proxy/{key}` — 解析 key，调用 `store.remove()`，返回 200/404/500
- [ ] 1.12 `proxy-server/src/main.rs`：创建 channel，传递 handle 给 API AppState
- [ ] 1.13 `cargo clippy -- -D warnings` 零警告
- [ ] 1.14 `cargo test` 零失败

**验证命令**：`cargo build -p proxy-api && cargo test -p proxy-api`

---

### F2: MCP stub 工具补全

- [ ] 2.1 `proxy-mcp/src/lib.rs`：`ProxyPoolMcp` 新增 `geoip: Option<Arc<Mutex<GeoIPLookup>>>` 和 `scheduler_handle: SchedulerHandle` 字段
- [ ] 2.2 `geoip_lookup` 工具：调用 `self.geoip.lock().await.lookup(host)`，返回结构化结果
- [ ] 2.3 `refresh_pool` 工具：调用 `self.scheduler_handle.refresh()`，返回实际结果
- [ ] 2.4 `ProxyPoolMcp` 构造函数签名更新
- [ ] 2.5 `proxy-server/src/main.rs`：传递 geoip 和 scheduler_handle 给 MCP server
- [ ] 2.6 `cargo clippy -- -D warnings` 零警告
- [ ] 2.7 `cargo test` 零失败

**验证命令**：`cargo build -p proxy-mcp && cargo test -p proxy-mcp`

---

### F5: 测试覆盖补全

- [ ] 5.1 `proxy-core`：store mock 测试（使用 redis-test 或真实 Redis）
- [ ] 5.2 `proxy-core`：validator mock 测试（mock HTTP target）
- [ ] 5.3 `proxy-core`：scheduler handle 测试（channel 通信）
- [ ] 5.4 `proxy-api`：每个端点 happy-path 测试（≥6 个）
- [ ] 5.5 `proxy-api`：delete_proxy 404 测试
- [ ] 5.6 `proxy-api`：refresh 实际触发测试
- [ ] 5.7 `proxy-mcp`：每个工具 happy-path 测试（≥8 个）
- [ ] 5.8 `proxy-mcp`：geoip_lookup 结构化结果测试
- [ ] 5.9 `proxy-gateway`：WarpChain 选择逻辑测试
- [ ] 5.10 `proxy-gateway`：HTTP CONNECT mock upstream 测试
- [ ] 5.11 `proxy-gateway`：SOCKS5 mock upstream 测试
- [ ] 5.12 `proxy-xray`：reconnect 逻辑测试
- [ ] 5.13 `proxy-xray`：outbound_sync 暂停/恢复测试
- [ ] 5.14 `cargo clippy -- -D warnings` 零警告
- [ ] 5.15 `cargo test` 全 workspace 零失败

**验证命令**：`cargo test --workspace && cargo clippy --workspace -- -D warnings`

---

## Review Gates

| Gate | 位置 | 验证 |
|------|------|------|
| G1 | F4 完成后 | `cargo build -p proxy-xray && cargo test -p proxy-xray` |
| G2 | F3 完成后 | `cargo build -p proxy-gateway && cargo test -p proxy-gateway` |
| G3 | F1+F2 完成后 | `cargo build && cargo test` |
| G4 | F5 完成后 | `cargo test --workspace && cargo clippy --workspace -- -D warnings` |

---

## Rollback Points

每个 F 完成后创建 git commit，出问题可单独 revert：
- F4 commit: `feat(xray): add gRPC auto-reconnect with exponential backoff`
- F3 commit: `feat(gateway): implement WarpChain chained proxy`
- F1 commit: `feat(api): wire up refresh and delete endpoints via scheduler channel`
- F2 commit: `feat(mcp): implement geoip_lookup and refresh_pool tools`
- F5 commit: `test: add comprehensive test coverage for all crates`
