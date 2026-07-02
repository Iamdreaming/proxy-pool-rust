# Phase 3 Design: 补全闭环

## 架构概览

Phase 3 不引入新架构层，只在现有 crate 内补全功能。核心设计挑战是 **跨 crate 通信**（API/MCP → Scheduler）和 **链式代理连接**。

---

## D1: Scheduler Channel 机制

### 问题

`proxy-api` 和 `proxy-mcp` 需要触发 `Scheduler::run_once()`，但 Scheduler 运行在 `proxy-server` 的 tokio task 中，API/MCP 运行在各自的 task 中。需要跨 task 通信。

### 方案：mpsc + oneshot 回调

```
API/MCP ──[Command]──► mpsc::Sender ──► Scheduler task
                                          │
                          Scheduler ──────┘
                              │
                          run_once() 完成
                              │
                          oneshot::Sender ──► API/MCP (等待结果)
```

### 接口设计

```rust
// proxy-core/src/scheduler.rs 新增

/// 调度器命令
pub enum SchedulerCommand {
    /// 触发一次刷新，通过 oneshot 返回结果
    Refresh { reply: oneshot::Sender<SchedulerResult> },
}

/// 刷新结果
pub struct SchedulerResult {
    pub fetched: usize,
    pub validated: usize,
    pub stored: usize,
    pub errors: usize,
}

// Scheduler 新增字段
pub struct Scheduler {
    // ... 现有字段 ...
    cmd_rx: Option<mpsc::Receiver<SchedulerCommand>>,
}

// Scheduler::run() 循环中增加 cmd_rx 轮询
// 使用 tokio::select! 同时监听 interval 和 cmd_rx
```

```rust
// proxy-core/src/scheduler.rs 新增

/// 用于外部触发 scheduler 的 handle
#[derive(Clone)]
pub struct SchedulerHandle {
    cmd_tx: mpsc::Sender<SchedulerCommand>,
}

impl SchedulerHandle {
    pub async fn refresh(&self) -> anyhow::Result<SchedulerResult> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx.send(SchedulerCommand::Refresh { reply: tx }).await?;
        Ok(rx.await?)
    }
}
```

### AppState 扩展

```rust
// proxy-api/src/routes.rs
pub struct AppState {
    pub store: Arc<ProxyStore>,
    pub xray_active_count: Arc<AtomicUsize>,
    pub scheduler_handle: SchedulerHandle,  // 新增
}
```

```rust
// proxy-mcp/src/lib.rs
pub struct ProxyPoolMcp {
    store: Arc<ProxyStore>,
    balancer: Option<Arc<WarpBalancer>>,
    geoip: Option<Arc<Mutex<GeoIPLookup>>>,  // 新增：用于 geoip_lookup
    scheduler_handle: SchedulerHandle,        // 新增：用于 refresh_pool
}
```

### proxy-server 组装变更

```rust
// proxy-server/src/main.rs
let (cmd_tx, cmd_rx) = mpsc::channel::<SchedulerCommand>(8);
let scheduler_handle = SchedulerHandle { cmd_tx };
let scheduler = Scheduler::new(/* ... */, Some(cmd_rx));

// 传递 scheduler_handle 给 API 和 MCP
let app_state = AppState {
    store: store.clone(),
    xray_active_count: xray_active_count.clone(),
    scheduler_handle: scheduler_handle.clone(),
};
```

---

## D2: DELETE /api/proxy/{key} 实现

### key 格式

Proxy 在 Redis 中的 member 是 JSON 序列化的 Proxy 结构。key 参数格式为 `{protocol}:{host}:{port}`（如 `http:1.2.3.4:8080`）。

### 实现逻辑

```rust
async fn delete_proxy(
    State(state): State<AppState>,
    Path(path): Path<DeleteProxyPath>,
) -> impl IntoResponse {
    // 解析 key: "protocol:host:port"
    let parts: Vec<&str> = path.key.splitn(3, ':').collect();
    if parts.len() != 3 {
        return (StatusCode::BAD_REQUEST, Json(SimpleResponse { status: "invalid key format, expected protocol:host:port".into() }));
    }
    let protocol = match Protocol::from_str(parts[0]) {
        Ok(p) => p,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(SimpleResponse { status: "invalid protocol".into() })),
    };
    let proxy = Proxy { host: parts[1].into(), port: parts[2].parse().unwrap_or(0), protocol: protocol.clone(), ..Default::default() };

    match state.store.mark_failed(&proxy).await {
        Ok(()) => (StatusCode::OK, Json(SimpleResponse { status: "ok".into() })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(SimpleResponse { status: format!("error: {e}") })),
    }
}
```

> 注：使用 `mark_failed` 而非直接删除，因为 store 目前没有 `remove` 公共方法。`mark_failed` 会降低代理分数，自然淘汰。如果需要立即移除，需在 `ProxyStore` 新增 `remove` 方法。

### ProxyStore 新增 remove 方法

```rust
// proxy-core/src/store.rs
pub async fn remove(&self, proxy: &Proxy) -> anyhow::Result<bool> {
    let key = format!("proxies:{}", proxy.protocol);
    let member = serde_json::to_string(proxy)?;
    let result: i64 = redis::cmd("ZREM")
        .arg(&key)
        .arg(&member)
        .query_async(&mut self.conn.clone())
        .await?;
    Ok(result > 0)
}
```

---

## D3: WarpChain 链式代理

### 连接流程

```
Client ──CONNECT──► Gateway
                       │
                   UpstreamSelector 返回 WarpChain { proxy, socks5_port }
                       │
                   Step 1: connect_via_socks5(proxy.addr, warp_socks5_addr)
                       │     → 建立 client → proxy → WARP 入口隧道
                       │
                   Step 2: 通过已建立的隧道，SOCKS5 CONNECT 到 target
                       │     → 建立 client → proxy → WARP → target 隧道
                       │
                   Step 3: 返回 200 给 client，双向转发
```

### 实现要点

1. **Step 1**：复用现有 `connect_via_socks5`，目标地址为 WARP SOCKS5 入口（`127.0.0.1:{socks5_port}`）
2. **Step 2**：在 Step 1 返回的 TcpStream 上执行 SOCKS5 握手，目标为实际 target
3. **超时**：每跳独立超时（默认 10s），总超时 = 2 × 单跳超时

### 代码位置

在 `proxy-gateway/src/upstream.rs` 新增 `connect_via_warp_chain` 函数：

```rust
pub async fn connect_via_warp_chain(
    proxy: &Proxy,
    warp_socks5_port: u16,
    target_addr: &str,
) -> anyhow::Result<tokio::net::TcpStream> {
    // Step 1: 通过 proxy 连接到 WARP SOCKS5 入口
    let warp_addr = format!("127.0.0.1:{warp_socks5_port}");
    let mut stream = connect_via_socks5(&proxy_addr(proxy), &warp_addr).await?;

    // Step 2: 在该 stream 上 SOCKS5 CONNECT 到 target
    socks5_handshake(&mut stream, target_addr).await?;

    Ok(stream)
}
```

`http_connect.rs` 和 `socks5.rs` 中 `WarpChain` 分支调用此函数。

---

## D4: xray gRPC 自动重连

### 方案：watch channel 传递连接状态

```rust
// proxy-xray/src/xray_client.rs

pub struct XrayClient {
    api_addr: String,
    api_port: u16,
    binary_path: String,
    grpc_client: Option<HandlerServiceClient<tonic::transport::Channel>>,
    connected: watch::Sender<bool>,   // 新增：广播连接状态
    connected_rx: watch::Receiver<bool>, // 新增：接收连接状态
}
```

### 重连逻辑

```rust
impl XrayClient {
    pub async fn ensure_connected(&mut self) -> Result<()> {
        if self.is_connected() {
            return Ok(());
        }
        self.connect().await
    }

    /// 后台重连循环（由 proxy-server spawn）
    pub async fn reconnect_loop(&mut self) {
        let mut backoff = Duration::from_secs(1);
        let max_backoff = Duration::from_secs(30);
        loop {
            if self.is_connected() {
                tokio::time::sleep(Duration::from_secs(5)).await;
                // 健康检查：尝试 gRPC 调用
                if !self.health_check().await {
                    let _ = self.connected.send(false);
                    self.grpc_client = None;
                }
                continue;
            }
            match self.connect().await {
                Ok(()) => {
                    backoff = Duration::from_secs(1);
                    let _ = self.connected.send(true);
                }
                Err(e) => {
                    tracing::warn!("xray gRPC reconnect failed: {e}, retry in {backoff:?}");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                }
            }
        }
    }
}
```

### outbound_sync 适配

```rust
// proxy-xray/src/outbound_sync.rs
impl OutboundSync {
    pub async fn sync_loop(&mut self, mut connected_rx: watch::Receiver<bool>) {
        let mut interval = tokio::time::interval(self.sync_interval);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if !*connected_rx.borrow_and_update() {
                        tracing::info!("xray disconnected, skipping sync");
                        continue;
                    }
                    if let Err(e) = self.sync_once().await {
                        tracing::error!("sync error: {e}");
                    }
                }
                _ = connected_rx.changed() => {
                    if *connected_rx.borrow() {
                        tracing::info!("xray reconnected, running immediate sync");
                        let _ = self.sync_once().await;
                    }
                }
            }
        }
    }
}
```

---

## D5: 测试策略

### proxy-api 测试

使用 `axum::test` + mock `ProxyStore`。由于 `ProxyStore` 不是 trait，需要：
- 方案 A：提取 `ProxyStore` trait，mock 实现（改动大）
- 方案 B：使用真实 Redis（docker，CI 友好）
- **选择方案 B**：集成测试用真实 Redis，单元测试覆盖路由逻辑

```rust
// crates/proxy-api/tests/api_test.rs
#[tokio::test]
async fn test_status_endpoint() {
    let app = create_test_app().await;
    let response = app.oneshot(
        Request::builder().uri("/api/status").body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
```

### proxy-mcp 测试

使用 `rmcp` 的 test helper 或直接调用 tool handler 函数。

### proxy-gateway 测试

- `upstream.rs`：扩展现有测试，覆盖 WarpChain 选择逻辑
- 协议处理：使用 `tokio::net::TcpListener` 模拟上游，测试 CONNECT/SOCKS5 流程

### proxy-core 测试

- `store.rs`：使用 mock Redis（redis-test crate 或真实 Redis）
- `validator.rs`：mock HTTP target server

### dev-dependencies

```toml
# 各 crate 的 [dev-dependencies]
tokio = { version = "1", features = ["test-util", "macros"] }
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
```

---

## 兼容性

- `SchedulerHandle` 是新增类型，不影响现有代码
- `ProxyStore::remove` 是新增方法，不影响现有调用
- `XrayClient` 新增 `connected` watch channel，构造函数签名变更（需更新 proxy-server）
- `ProxyPoolMcp` 构造函数新增参数（需更新 proxy-server）
- `AppState` 新增字段（需更新 proxy-server）

所有公共 API 变更都是**扩展性变更**（新增字段/参数），不破坏现有调用。proxy-server 作为唯一消费者，同步更新即可。

---

## 回滚策略

每个 Feature (F1-F5) 独立提交，任一出问题可单独 revert：
- F4: revert xray_client 变更，恢复单次 connect 行为
- F3: revert gateway 变更，恢复 WarpChain 501 行为
- F1/F2: revert channel 机制，恢复 stub 行为
- F5: 测试代码 revert 不影响运行时
