# aggregator-enhancement — Technical Design

## Architecture Overview

6 个需求按依赖关系分为 4 层，每层可独立实现和验证：

```
Layer 0 (基础设施)
  R4: 来源可信度体系        ← 其他所有 R 依赖 SourceOrigin 标签
  R3: 订阅元数据追踪        ← R6 签到续期依赖订阅健康状态

Layer 1 (新爬源)
  R1: Telegram 频道爬源     ← 独立，依赖 R4 的 SourceOrigin::Telegram

Layer 2 (机场生态)
  R2: 机场自动注册          ← 依赖 R4 的 SourceOrigin::Airport + R3 的元数据
  R6: 自动签到续期          ← 依赖 R2 的注册信息 + R3 的元数据

Layer 3 (智能路由)
  R5: 节点能力标记          ← 独立，依赖 proxy-core 的 Validator + gateway
```

## Module Boundaries

### 新增文件

```
crates/proxy-sub/src/
  discover/
    telegram.rs            # R1: TelegramDiscover
    airport.rs             # R2: AirportDiscover
  airport/
    mod.rs                 # R2: 机场注册核心逻辑
    panel.rs               # R2: 面板类型探测 + 注册要求
    register.rs            # R2: 注册 + 免费套餐领取
    email.rs               # R2: Cloudflare 临时邮箱集成
  subscription_meta.rs     # R3: 订阅元数据解析 + 存储
  checkin.rs               # R6: 签到 + 续期

crates/proxy-core/src/
  source_origin.rs         # R4: SourceOrigin 枚举 + 可信度窗口
  capability.rs            # R5: CapabilityTag + 能力测试
  validator_ext.rs         # R5: 验证后能力测试扩展

crates/proxy-gateway/src/
  capability_route.rs      # R5: 基于能力标签的路由增强
```

### 修改文件

```
crates/proxy-core/src/config.rs          # + TelegramConfig, AirportConfig, CapabilityConfig
crates/proxy-sub/src/discover/mod.rs     # + pub mod telegram; pub mod airport;
crates/proxy-sub/src/ops.rs              # R3/R4: 元数据追踪 + 可信度融入 gate
crates/proxy-sub/src/refresh.rs          # R1/R2: build_discoverers 增加 Telegram/Airport
crates/proxy-core/src/scheduler.rs       # R5: revalidate 时触发能力测试
crates/proxy-gateway/src/upstream.rs     # R5: 选择节点时考虑能力标签
crates/proxy-api/src/routes.rs           # R3/R6: 新增 API 端点
crates/proxy-mcp/src/lib.rs              # R3/R6: 新增 MCP 工具
```

## Data Flow

### R1: Telegram 爬取流程

```
配置 telegram.channels
       │
       ▼
TelegramDiscover::discover()
       │
       ├─ 对每个 enabled channel:
       │   GET https://t.me/s/{channel}
       │   解析 HTML → 提取消息文本
       │   │
       │   ├─ 正则匹配订阅 API URL → 加入 urls 列表
       │   ├─ 正则匹配协议直链 → 加入 direct_links 列表
       │   └─ 分页: ?before={id} → 递归爬取
       │
       ▼
返回 Vec<String> (订阅 URL)
       │
       ▼
现有 ops.rs::run_entry 流程:
  discover_urls → fetch → parse_subscription → partition → store
```

协议直链的特殊处理：`TelegramDiscover` 内部将 `vmess://` 等直链转换为 data URI（`vmess://xxx` → 订阅内容为单行的 "订阅"），使其能走 `Base64UriParser` 解析。或者更简单的方式——在 `discover()` 中将直链标记为特殊类型，由 `ops.rs` 分发到 parser。

**设计决策**：`discover()` 返回 `Vec<String>` 不变，协议直链以 `vmess://xxx` 形式返回。在 `ops.rs::run_entry` 中增加一步：对 `discover()` 返回的 URL 列表做预处理，识别协议直链，直接调用 `parse_uri` 解析后走 `partition`，跳过 `fetch` 步骤。

### R2: 机场注册流程

```
配置 airport.aggregator_sites
       │
       ▼
AirportDiscover::discover()
       │
       ├─ 1. 发现机场域名:
       │   爬取聚合站页面 → 正则提取域名列表
       │   去重 + 过滤已知域名
       │
       ├─ 2. 面板探测:
       │   对每个域名: GET /guest/comm/config
       │   解析 RegisterRequire (verify/invite/recaptcha/whitelist)
       │   过滤不可注册站点
       │
       ├─ 3. 自动注册:
       │   生成临时邮箱 (Cloudflare worker)
       │   POST /api/v1/passport/auth/register
       │   如需验证码: 监听邮箱 → 提取验证码 → 重新注册
       │
       ├─ 4. 免费套餐:
       │   GET /api/v1/user/server/fetch → 获取 plan_id
       │   POST order → 领取免费套餐
       │
       ├─ 5. 提取订阅 URL:
       │   GET /api/v1/client/subscribe?token={token}
       │
       └─ 6. 持久化到 Redis:
           key: airport:accounts:{domain}
           value: { domain, email, password, token, sub_url, registered_at }
```

### R3: 订阅元数据追踪

```
ops.rs::run_entry 中 fetch 成功后:
       │
       ▼
parse_subscription_userinfo(response_headers)
       │
       ├─ 解析 header: "upload=xxx; download=xxx; total=xxx; expire=xxx"
       ├─ 计算: remaining_bytes = total - upload - download
       │         remaining_days = (expire - now) / 86400
       │         health = remaining_bytes / total * remaining_days / expire_days
       │
       ▼
SubscriptionMeta {
  upload: u64, download: u64, total: u64,
  expire: Option<i64>,
  remaining_ratio: f64,
  remaining_days: Option<f64>,
  health: f64,  // 0.0 ~ 1.0
}
       │
       ├─ 存储: Redis SET subscription:meta:{source_id} + TTL
       │
       ├─ 判定:
       │   remaining_ratio < 0.01 OR remaining_days < 0.5 → mark expired, skip
       │   remaining_ratio < 0.1 OR remaining_days < 3 → mark low_health
       │
       └─ 报告: 写入 SubscriptionSourceReport.metadata
```

### R4: 来源可信度

```
SourceOrigin 枚举:
  Owned(∞), Manual(∞), GitHub(20), Airport(7),
  Aggregator(10), Telegram(3)

每个 SubscriptionSourceEntry 新增字段:
  origin: SourceOrigin
  last_success_at: Option<DateTime<Utc>>
  consecutive_failures: u32

降级逻辑 (在 recommend_apply 中):
  days_since_success = (now - last_success_at).days

  match origin.expiry_days() {
    ∞ => 不降级,  // Owned/Manual
    d if days_since_success > d * 2 => Reject,
    d if days_since_success > d => Review,
    _ => 正常 gate 逻辑
  }

与 circuit breaker 叠加:
  circuit breaker: 秒~分钟级, 单次 refresh 内生效
  可信度降级: 天级, 跨 refresh 周期生效
  两者独立判断, 任一触发即生效
```

### R5: 节点能力标记

```
scheduler.rs::revalidate_existing 完成后:
       │
       ▼
get_top_candidates(store, protocol, k=8)
       │
       ├─ 对每个候选节点, 并行执行能力测试:
       │   capability_test(proxy, targets)
       │     ├─ 通过代理 GET chat.openai.com/favicon.ico → 200? → ChatGPT tag
       │     ├─ 通过代理 GET api.openai.com/v1/engines → 401? → OpenAI tag
       │     └─ 通过代理 GET youtube.com/favicon.ico → 200? → YouTube tag
       │
       ▼
Redis SADD proxy:capabilities:{proxy_key} {tag1} {tag2} ...

网关路由增强 (upstream.rs::try_pool_candidates):
  当目标域名匹配 openai/chatgpt 时:
    1. 先从带 ChatGPT 标签的节点中选
    2. 无可用标签节点时, 回退到普通选择
```

### R6: 签到续期

```
subscription_ops_loop 增加签到步骤:
  每隔 checkin_interval_sec:
    │
    ├─ 从 Redis 加载 airport:accounts:* 列表
    │
    ├─ 对每个已注册站点:
    │   POST {domain}/user/checkin (cookie/auth)
    │   记录签到结果到 airport:checkin:{domain}
    │
    ├─ 检查订阅元数据 (R3):
    │   remaining_ratio >= 0.8 OR remaining_days <= 5
    │     → 触发续期: order_plan → 重新领取免费流量
    │
    └─ 更新 airport:accounts:{domain} 中的 token/sub_url
```

## Configuration Schema

```yaml
# 新增配置项 (在 proxy-core/src/config.rs 中定义)

subscription:
  telegram:
    enabled: false
    channels:
      - name: "proxy_list_channel"
        pages: 1          # 爬取页数
        include: ""        # 正则, 仅保留匹配的链接
        exclude: ""        # 正则, 排除匹配的链接
        enabled: true
    timeout_sec: 30

  airport:
    enabled: false
    aggregator_sites:      # 可配置聚合站, 提供默认值
      - url: "https://example.com/free-airports"
        format: "html"     # html/json/text
    cloudflare_worker_url: ""  # Cloudflare 临时邮箱 worker 地址
    max_concurrent: 3      # 并发注册数
    timeout_sec: 30

capabilities:
  enabled: true
  test_on_validate: false     # 首次验证时不测试
  test_on_revalidate: true    # 重验时测试
  top_k: 8                    # 仅对 top-K 节点测试
  targets:
    - name: "ChatGPT"
      url: "https://chat.openai.com/favicon.ico"
      expected_status: 200
    - name: "OpenAI"
      url: "https://api.openai.com/v1/engines"
      expected_status: 401

checkin:
  enabled: false
  interval_sec: 86400         # 24h
```

## Redis Key Schema

```
# R2: 机场注册信息
airport:accounts:{domain}       → Hash { email, password, token, sub_url, registered_at, panel_type }
airport:accounts                → Set of registered domains

# R3: 订阅元数据
subscription:meta:{source_id}   → Hash { upload, download, total, expire, remaining_ratio, remaining_days, health }

# R4: 来源可信度 (嵌入 SubscriptionSourceEntry, 通过 ops state 管理)

# R5: 节点能力标签
proxy:capabilities:{proxy_key}  → Set of CapabilityTag strings

# R6: 签到结果
airport:checkin:{domain}        → Hash { last_checkin_at, result, message }
```

## Compatibility & Migration

- **向后兼容**：所有新增配置项默认关闭（`enabled: false`），不影响现有部署
- **Redis schema**：新增 key 不影响现有 key；`proxy:capabilities:*` 使用独立命名空间
- **API 兼容**：现有 API 端点不变，新增端点为扩展
- **Discover trait**：不变，新实现遵循现有接口
- **MCP 工具**：新增工具不修改现有工具签名

## Trade-offs

| 决策 | 选择 | 放弃的替代方案 | 理由 |
|------|------|---------------|------|
| 协议直链处理 | `discover()` 返回直链, ops.rs 预处理 | 新增 Discover trait 返回类型 | 保持 trait 接口不变，向后兼容 |
| 临时邮箱 | Cloudflare Worker | Mail.tm API / 自建邮箱 | 用户已有 CF 邮箱，稳定可控 |
| 聚合站列表 | 可配置 + 默认值 | 纯硬编码 | 站点变动频繁，需可配置 |
| 能力测试频率 | 仅 top-K + 重验 | 每次验证都测试 | 性能优先，top-K 覆盖网关实际使用 |
| 可信度 vs circuit breaker | 叠加分层 | 替代 | 作用时间尺度不同，互补 |
| 签到调度 | 嵌入 subscription loop | 独立 cron | 复用现有调度基础设施，减少运维复杂度 |

## Rollback

- 每个需求可独立回滚：`enabled: false` 即可关闭
- Redis 新增 key 使用 TTL 自动过期，无需手动清理
- 机场注册信息如需清除：`DEL airport:accounts:*`
- 能力标签如需清除：`DEL proxy:capabilities:*`
