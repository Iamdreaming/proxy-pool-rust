# 借鉴 aggregator 增强代理源与订阅管理

## Goal

借鉴 [wzdnzd/aggregator](https://github.com/wzdnzd/aggregator) 的核心能力，增强 proxy-pool-rust 的代理源发现、订阅生命周期管理和节点质量标记，使系统能自动从更多渠道发现节点、智能管理订阅健康度、精准匹配节点能力。

## Background

当前 proxy-pool-rust 的节点获取依赖两条路径：
1. **Free-list fetchers**（10 个固定源，仅提供 HTTP/SOCKS 明文代理）
2. **Subscription discovers**（StaticUrl / GitHubSearch / AggregatorDiscover，需手动配置或 GitHub API 搜索）

相比之下，aggregator 具备以下我们缺失的核心能力：
- **Telegram 频道爬取**：免费节点最活跃的来源，通过 `t.me/s/{channel}` 公开页面无需 API
- **机场自动注册**：发现机场站 → 自动注册 → 领免费套餐 → 获取订阅链接，形成闭环
- **订阅元数据追踪**：解析 `subscription-userinfo` 头，跟踪流量/到期
- **来源可信度体系**：不同来源不同过期窗口，自动淘汰
- **节点能力标记**：ChatGPT/OpenAI 可达性实测标记
- **自动签到续期**：延长免费订阅有效期

## Requirements

### R1: Telegram 频道爬源（P0）

新增 `TelegramDiscover`，实现现有 `Discover` trait，从 Telegram 频道公开页面提取订阅链接和协议链接。

- **R1.1** 配置项：`telegram.channels: Vec<TelegramChannelConfig>`，每个 channel 含 `name`、`pages`（爬取页数，默认 1）、`include`/`exclude`（正则过滤）、`enabled`
- **R1.2** 爬取机制：GET `https://t.me/s/{channel}`，解析 HTML 提取消息内容；支持 `?before={id}` 分页
- **R1.3** 链接提取：识别三种模式——订阅 API URL（`/api/v1/client/subscribe?token=`、`/link/`、`/sub/`）、协议直链（`vmess://`、`trojan://`、`ss://`、`vless://` 等）、subconverter 嵌套链接
- **R1.4** 协议直链处理：提取的 `vmess://`、`trojan://` 等单节点链接走 `Base64UriParser` 解析后进入 `partition` 流程（Basic → pool，Encrypted → PendingStore）
- **R1.5** 分页探测：首次爬取时探测 `canonical` link 获取最早 post id，按步长回溯
- **R1.6** 去重：URL 级别去重，与现有 `discover_urls` 去重逻辑一致
- **R1.7** 错误处理：网络失败记录到 `SubscriptionSourceReport.errors`，不中断其他频道

### R2: 机场自动注册（P1）

新增 `AirportDiscover`，自动发现机场站、注册免费账号、获取订阅链接。

- **R2.1** 机场站发现：爬取可配置的聚合站列表获取机场域名，提供默认站点
- **R2.2** 面板类型探测：嗅探 `/guest/comm/config` 判断 v2board/sspanel 面板类型和注册要求（邮箱验证、邀请码、recaptcha）
- **R2.3** 自动注册：根据注册要求自动填写注册表单，使用 Cloudflare 临时邮箱接收验证码
- **R2.4** 免费套餐领取：注册后自动获取免费套餐（`order_plan`）
- **R2.5** 订阅链接获取：注册成功后提取订阅 URL，作为新的 subscription source
- **R2.6** 注册门槛过滤：跳过需要邀请码、recaptcha 或受限邮箱白名单的站点
- **R2.7** 持久化：注册成功的站点信息（域名、账号、订阅 URL）持久化到 Redis，下次 refresh 可复用
- **R2.8** API 前缀自适应：支持 `/api/v1/` 和 `/api?scheme=` 两种 v2board API 风格

### R3: 订阅元数据追踪（P2）

在订阅刷新流程中解析和追踪订阅的流量/到期信息。

- **R3.1** 解析 `subscription-userinfo` 响应头：提取 `upload`、`download`、`total`（bytes）和 `expire`（unix timestamp）
- **R3.2** 计算订阅健康度：剩余流量比例、剩余有效天数
- **R3.3** 自动跳过：已耗尽流量或已过期的订阅在 refresh 时跳过 fetch，标记为 `expired`
- **R3.4** 存储：订阅元数据存入 Redis（key `subscription:meta:{source_id}`），含 TTL 等于订阅到期时间
- **R3.5** API/MCP 暴露：在 `/api/subscriptions/sources` 和 MCP `subscription_sources` 中展示流量/到期状态

### R4: 来源可信度体系（P3）

为不同来源的订阅链接设置不同的过期/淘汰策略。与现有 circuit breaker 叠加分层：circuit breaker 负责短期保护（秒~分钟级），可信度负责长期淘汰（天~周级）。

- **R4.1** 来源分类枚举 `SourceOrigin`：`Owned`(∞)、`Telegram`(3d)、`GitHub`(20d)、`Aggregator`(10d)、`Airport`(7d)、`Manual`(∞)
- **R4.2** 可信度窗口：每个来源有对应的过期天数，超过窗口未刷新成功的订阅自动降级（Apply → Review → Reject）
- **R4.3** 融入现有 gate：`recommend_apply` 中考虑来源可信度——高可信度源放宽阈值，低可信度源收紧
- **R4.4** 降级规则：低可信度源连续 3 天未刷新成功 → Apply 降 Review；7 天 → Reject。高可信度源放宽至 7 天/14 天

### R5: 节点能力标记（P4）

在验证阶段增加特定服务的可达性标记。仅对 top-K 候选节点在首次验证和定期重验时测试。

- **R5.1** 新增能力标签类型 `CapabilityTag`：`ChatGPT`、`OpenAI`、`YouTube`、`Google`
- **R5.2** 验证扩展：首次验证通过后 + `revalidate_existing` 重验时，对 top-K 候选节点追加能力测试（通过代理访问 chat.openai.com/favicon.ico 期望 200、api.openai.com/v1/engines 期望 401）
- **R5.3** 标记存储：能力标签存入 Redis（key `proxy:capabilities:{key}`），Set 类型
- **R5.4** 网关路由增强：`UpstreamSelector` 在选择海外节点时优先选择带 `ChatGPT` 标签的节点访问 openai 域名
- **R5.5** 配置项：`capabilities.enabled`（默认 true）、`capabilities.test_on_validate`（默认 false）、`capabilities.test_on_revalidate`（默认 true）、`capabilities.targets: Vec<CapabilityTarget>`（URL + 期望状态码 + 标签名）
- **R5.6** 性能约束：能力测试与主验证并行，不增加总延迟；仅对 top-K（默认 8）候选节点测试

### R6: 自动签到续期（P5）

对已注册的机场站执行自动签到和流量续期。

- **R6.1** 签到流程：POST `/user/checkin`，使用注册时保存的 cookie/auth
- **R6.2** 流量续期：当订阅流量使用 ≥80% 或有效期 ≤5 天时，触发 `order_plan` 重新领取免费流量
- **R6.3** 调度：在 subscription refresh loop 中增加签到步骤，可配置间隔（默认 24h）
- **R6.4** 结果记录：签到/续期结果写入 Redis，API/MCP 可查

## Acceptance Criteria

### AC1: Telegram Discover
- [ ] `TelegramDiscover` 实现 `Discover` trait，`name()` 返回 `"telegram"`
- [ ] 配置 `telegram.channels` 后，`discover()` 返回从指定频道提取的订阅 URL 列表
- [ ] 提取的协议直链（vmess:// 等）经解析后正确进入 pool 或 PendingStore
- [ ] 分页爬取能获取历史消息中的链接
- [ ] 网络失败不中断其他频道的爬取
- [ ] `cargo test` 零失败，`cargo clippy -- -D warnings` 零警告

### AC2: Airport Auto-Registration
- [ ] `AirportDiscover` 实现 `Discover` trait，`name()` 返回 `"airport"`
- [ ] 能从聚合站发现机场域名，探测面板类型，过滤不可注册站点
- [ ] 对可注册站点完成自动注册 + 免费套餐领取 + 订阅链接提取
- [ ] 注册信息持久化到 Redis，refresh 时复用已有账号
- [ ] 需要邀请码/recaptcha/受限邮箱的站点被正确跳过
- [ ] `cargo test` 零失败，`cargo clippy -- -D warnings` 零警告

### AC3: Subscription Metadata
- [ ] 刷新订阅时解析 `subscription-userinfo` 头，提取流量/到期信息
- [ ] 已耗尽/过期订阅被跳过并标记
- [ ] API 和 MCP 中展示订阅健康状态
- [ ] `cargo test` 零失败，`cargo clippy -- -D warnings` 零警告

### AC4: Source Credibility
- [ ] 每个订阅源携带 `SourceOrigin` 标签和对应过期窗口
- [ ] 超过可信度窗口的源被降级或淘汰
- [ ] `recommend_apply` 考虑来源可信度
- [ ] 与 circuit breaker 叠加分层，不冲突
- [ ] `cargo test` 零失败，`cargo clippy -- -D warnings` 零警告

### AC5: Node Capability Tagging
- [ ] 首次验证和重验时，top-K 节点执行能力测试并标记
- [ ] 能力标签存入 Redis，网关路由可按标签选择节点
- [ ] 访问 openai 域名时优先选择 ChatGPT 标签节点
- [ ] 能力测试不增加主验证延迟
- [ ] `cargo test` 零失败，`cargo clippy -- -D warnings` 零警告

### AC6: Auto Check-in/Renewal
- [ ] 已注册机场站按配置间隔自动签到
- [ ] 流量不足或即将过期时自动续期
- [ ] 签到/续期结果可在 API/MCP 查询
- [ ] `cargo test` 零失败，`cargo clippy -- -D warnings` 零警告

## Technical Notes

- 所有新 Discover 实现遵循现有 `Discover` trait（`name() + discover()` → `Vec<String>`）
- Telegram 爬取使用 reqwest（项目已有依赖），解析 HTML 使用 `scraper` crate（`free_proxy_list` fetcher 已用）
- 机场注册使用 Cloudflare 临时邮箱接收验证码，配置项 `email.cloudflare_worker_url`
- 机场聚合站列表可配置（`airport.aggregator_sites`），提供默认值
- 来源可信度融入现有 `SubscriptionSourceQualityMetrics` + `recommend_apply` 体系，与 circuit breaker 叠加分层
- 节点能力标记仅对 top-K 候选节点在首次验证/重验时测试，使用现有 reqwest `Proxy::all` 机制
- 签到/续期复用机场注册时保存的认证信息

## Out of Scope

- 多格式输出（Clash/V2Ray/SingBox）— 我们是网关模式，不需要生成客户端订阅文件
- 外部存储推送（Gist/PasteGG）— 架构不同
- Subconverter 集成 — 我们已有自己的 parser 体系
- 通用 HTML 爬虫框架 — 仅针对 Telegram 和聚合站
- Google/Yandex/Twitter 搜索爬源 — 投入产出比低，容易被封
