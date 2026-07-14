# aggregator-enhancement — Implementation Plan

## Execution Order

按依赖分层实现，每层完成后验证再进入下一层。

### Phase 0: 基础设施 (R4 + R3)

**R4: 来源可信度体系** — 其他所有 R 依赖 `SourceOrigin` 标签

- [ ] 0.1 在 `proxy-core/src/source_origin.rs` 定义 `SourceOrigin` 枚举和 `expiry_days()` 方法
- [ ] 0.2 在 `SubscriptionSourceEntry` 中新增 `origin: SourceOrigin`、`last_success_at: Option<DateTime<Utc>>`、`consecutive_failures: u32` 字段
- [ ] 0.3 在 `SubscriptionSourceDescriptor` 中新增 `origin` 字段，配置中为每种 source kind 映射默认 origin
- [ ] 0.4 修改 `recommend_apply`：根据 `origin.expiry_days()` 和 `days_since_success` 调整阈值
- [ ] 0.5 修改 `run_entry`：成功时更新 `last_success_at`，失败时递增 `consecutive_failures`
- [ ] 0.6 单元测试：各 origin 的降级逻辑、与 circuit breaker 叠加场景
- [ ] **验证**: `cargo test -p proxy-sub` + `cargo clippy`

**R3: 订阅元数据追踪** — R6 签到续期依赖订阅健康状态

- [ ] 0.7 在 `proxy-sub/src/subscription_meta.rs` 定义 `SubscriptionMeta` 结构体和 `parse_subscription_userinfo()` 函数
- [ ] 0.8 在 `proxy-sub/src/subscription_meta.rs` 实现 `SubscriptionMetaStore`（Redis 存储，key `subscription:meta:{source_id}`）
- [ ] 0.9 修改 `ops.rs::run_entry`：fetch 成功后调用 `parse_subscription_userinfo`，存储元数据
- [ ] 0.10 修改 `ops.rs::run_entry`：refresh 前检查元数据，expired 的跳过 fetch
- [ ] 0.11 修改 `SubscriptionSourceReport` 新增 `metadata: Option<SubscriptionMeta>` 字段
- [ ] 0.12 修改 API `/api/subscriptions/sources` 响应包含元数据
- [ ] 0.13 修改 MCP `subscription_sources` 工具展示元数据
- [ ] 0.14 单元测试：header 解析、过期判定、Redis 存取
- [ ] **验证**: `cargo test -p proxy-sub -p proxy-api -p proxy-mcp` + `cargo clippy`

### Phase 1: Telegram 爬源 (R1)

- [ ] 1.1 在 `proxy-core/src/config.rs` 新增 `TelegramConfig` 和 `TelegramChannelConfig`
- [ ] 1.2 在 `proxy-sub/src/discover/telegram.rs` 实现 `TelegramDiscover`
  - `crawl_channel(channel, pages)` → GET t.me/s/{channel} → 解析 HTML
  - `extract_links(html)` → 正则提取订阅 URL + 协议直链
  - `detect_pagination(html)` → 提取 canonical before id
  - `crawl_page(url)` → 单页爬取
- [ ] 1.3 修改 `ops.rs::run_entry`：预处理 `discover()` 返回的协议直链
  - 识别 `vmess://`、`trojan://`、`ss://`、`vless://` 前缀
  - 直接调用 `parse_uri` 解析 → `partition` → store
  - 跳过 fetch 步骤
- [ ] 1.4 修改 `refresh.rs::build_discoverers`：新增 `TelegramDiscover` 构建
- [ ] 1.5 修改 `ops.rs`：新增 `SubscriptionSourceKind::Telegram` 和对应 descriptor
- [ ] 1.6 为 `TelegramDiscover` 的 `SubscriptionSourceDescriptor.origin` 设置 `SourceOrigin::Telegram`
- [ ] 1.7 单元测试：HTML 解析、链接提取、分页、协议直链处理
- [ ] **验证**: `cargo test -p proxy-sub` + `cargo clippy` + 配置真实频道端到端测试

### Phase 2: 机场生态 (R2 + R6)

**R2: 机场自动注册**

- [ ] 2.1 在 `proxy-core/src/config.rs` 新增 `AirportConfig`、`AggregatorSiteConfig`
- [ ] 2.2 在 `proxy-sub/src/airport/email.rs` 实现 Cloudflare 临时邮箱集成
  - `create_temp_email(worker_url)` → 获取临时邮箱地址
  - `monitor_inbox(worker_url, account, timeout)` → 轮询收件箱获取验证码
  - `extract_verification_code(body)` → 正则提取 6 位验证码
- [ ] 2.3 在 `proxy-sub/src/airport/panel.rs` 实现面板探测
  - `detect_panel_type(domain)` → GET /guest/comm/config → RegisterRequire
  - `is_registerable(require)` → 过滤 invite/recaptcha/whitelist 限制
- [ ] 2.4 在 `proxy-sub/src/airport/register.rs` 实现注册流程
  - `register(domain, email, password, email_code, invite_code)` → POST /api/v1/passport/auth/register
  - `order_free_plan(domain, token)` → 领取免费套餐
  - `get_subscribe_url(domain, token)` → 提取订阅 URL
- [ ] 2.5 在 `proxy-sub/src/airport/mod.rs` 组装完整流程
  - `discover_airport_domains(sites)` → 爬取聚合站获取域名
  - `register_airport(domain, email_config)` → 完整注册流程
  - `persist_account(redis, account)` → 持久化到 Redis
  - `load_accounts(redis)` → 加载已注册账号
- [ ] 2.6 在 `proxy-sub/src/discover/airport.rs` 实现 `AirportDiscover`
  - `discover()` → 加载已注册账号的 sub_url + 发现新站点注册
- [ ] 2.7 修改 `refresh.rs::build_discoverers`：新增 `AirportDiscover` 构建
- [ ] 2.8 修改 `ops.rs`：新增 `SubscriptionSourceKind::Airport`，origin = `SourceOrigin::Airport`
- [ ] 2.9 单元测试：面板探测、注册流程、Redis 持久化
- [ ] **验证**: `cargo test -p proxy-sub` + `cargo clippy`

**R6: 自动签到续期**

- [ ] 2.10 在 `proxy-sub/src/checkin.rs` 实现签到逻辑
  - `checkin(domain, auth)` → POST /user/checkin
  - `renew_if_needed(domain, auth, meta)` → 检查元数据，触发续期
- [ ] 2.11 修改 `subscription_ops_loop`：增加签到步骤，按 `checkin_interval_sec` 间隔执行
- [ ] 2.12 修改 API：新增 `/api/airports/checkin` 端点
- [ ] 2.13 修改 MCP：新增 `airport_checkin` 工具
- [ ] 2.14 单元测试：签到流程、续期触发条件
- [ ] **验证**: `cargo test -p proxy-sub -p proxy-api -p proxy-mcp` + `cargo clippy`

### Phase 3: 智能路由 (R5)

- [ ] 3.1 在 `proxy-core/src/capability.rs` 定义 `CapabilityTag` 枚举和 `CapabilityTarget` 配置
- [ ] 3.2 在 `proxy-core/src/capability.rs` 实现 `CapabilityStore`（Redis Set 存取）
- [ ] 3.3 在 `proxy-core/src/validator_ext.rs` 实现能力测试
  - `test_capabilities(proxy, targets)` → 并行测试各 target
  - `test_single_capability(proxy, target)` → 通过代理访问 URL，检查状态码
- [ ] 3.4 修改 `scheduler.rs::revalidate_existing`：重验后对 top-K 节点调用 `test_capabilities`
- [ ] 3.5 修改 `scheduler.rs` 首次验证流程：`test_on_validate` 为 true 时对通过验证的节点测试
- [ ] 3.6 在 `proxy-gateway/src/capability_route.rs` 实现基于能力标签的路由
  - `select_by_capability(candidates, required_tag)` → 优先选带标签的节点
- [ ] 3.7 修改 `upstream.rs::try_pool_candidates`：openai 域名时优先选 ChatGPT 标签节点
- [ ] 3.8 修改 API：新增 `/api/proxies/capabilities` 端点
- [ ] 3.9 修改 MCP：新增 `proxy_capabilities` 工具
- [ ] 3.10 单元测试：能力测试逻辑、Redis 存取、路由选择
- [ ] **验证**: `cargo test` + `cargo clippy` + 端到端路由测试

### Final Integration

- [ ] 4.1 全量 `cargo test` 零失败
- [ ] 4.2 全量 `cargo clippy -- -D warnings` 零警告
- [ ] 4.3 配置文件示例更新（config.example.yaml 或 README）
- [ ] 4.4 更新 CLAUDE.md 中的项目结构说明
- [ ] 4.5 提交: `feat(sub,core,gateway): add aggregator-inspired enhancements`

## Validation Commands

```bash
# 每个阶段完成后
cargo test -p proxy-sub -p proxy-core -p proxy-gateway -p proxy-api -p proxy-mcp
cargo clippy -- -D warnings

# 最终集成
cargo test
cargo clippy -- -D warnings
cargo build
```

## Risky Files / Rollback Points

| 文件 | 风险 | 回滚策略 |
|------|------|---------|
| `proxy-sub/src/ops.rs` | 修改核心 run_entry 流程 | 保留原始逻辑分支，新功能通过 feature flag 控制 |
| `proxy-core/src/config.rs` | 新增配置项可能影响序列化 | 所有新字段 `#[serde(default)]` |
| `proxy-gateway/src/upstream.rs` | 修改节点选择逻辑 | 能力标签选择为 fallback，无标签时回退原逻辑 |
| `proxy-sub/src/discover/telegram.rs` | HTML 解析依赖 t.me 页面结构 | 正则宽松匹配，解析失败返回空不 panic |

## Sub-agent Context

implement.jsonl 和 check.jsonl 将在 `task.py start` 前填充，包含每个 R 的关键文件路径、trait 签名、配置结构体定义。
