# PRD: 高质量海外稳定代理获取（Parent）

## Goal

在**不引入付费代理供应商 / 付费 VPS**的前提下，把系统从“免费列表堆量”转向**可验证的海外稳定链路**：
优先让订阅 + xray（及已有 WARP 兜底）成为海外出口主力，并用更严的准入、评分、清理与取用门槛，把 free HTTP 池降为低优先级候选。

## User Value

- 网关在访问海外目标时，能稳定选到可用出口，而不是 10s+ 的 free best-proxy。
- 运营能看见：哪些源值得保留、哪些节点验证失败、哪些该清理。
- 试用/自有订阅可**安全接入**（preview → apply → 验证），而不是靠黑盒自动注册。

## Background / Confirmed Facts

### Live pool (2026-07-11)

- Pool roughly: HTTP ~646, SOCKS5 ~312, total ~968（波动中）。
- Quality summary: `excellent=0`, `good≈3`, `fair≈1200`, `poor≈948`。
- `recent_success_rate ≈ 0.37`；`stale_proxies` 很高；top failure reason 几乎全是 `validation_failed`。
- `get_best_proxy(http)` 返回过 KE 节点 **~11s latency**，score 仍约 0.5（elite + 多次成功，但 latency 归一化已饱和为 0）。

### Free fetchers

- 大量 free list 一次可解析 2k–3k 候选（TheSpeedX / Databay / Proxifly / VPSLab 等）。
- 部分源 empty/error：ProxyScrape、FreeProxyList、GeoNode。
- Free source 主要贡献**噪音候选**，不是海外稳定主力。

### Subscription + xray

- 订阅源已启用，例如：
  - `static-url-1`：airport-free v2ray 列表，parsed ~1354，encrypted stored ~1302，grade 高。
  - `static-url-2`：TopChina clash_sub，parsed ~267 basic。
- xray：`enabled=true`，`active_nodes=0`，`failed_nodes` 数百；近期错误以 `xray validation failed` 为主，另有 HTTP transport 已移除等 config 构建失败。
- 已有 in_progress 任务 `07-08-vless-xray-validation`：目标是 VLESS 一等公民 + xray 激活前对验证目标做可用性检查（YouTube 等海外场景）。

### Scoring / retention（`docs/score-retention.md`）

- `score = 0.5*latency_norm + 0.3*success_rate + 0.2*anonymity_norm`
- `latency_norm = clamp((2000 - ms)/2000, 0, 1)` → **>2s 延迟贡献恒为 0**
- 默认 `min_score=0.1`，几乎不过滤
- trend / quality_history 目前只做 explainability，不进 score
- `cleanup_low_score_proxies` 默认 dry-run，尚无后台自动清理

### Existing related tasks

| Task | Status | Relation |
|------|--------|----------|
| `07-08-vless-xray-validation` | in_progress | 海外加密节点可用性核心 |
| `07-07-business-target-routing-validation-v1` | in_progress | 业务目标路由验证 |
| `07-07-fetcher-validator-quality` | paused | 源诊断已部分落地 |
| `07-07-proxy-quality-recommendations-dry-run` | paused | 推荐淘汰（只读） |
| `07-07-free-source-expansion-v2` | in_progress | 只增供给，质量上限低 |
| `07-07-warp-ops-enhancement` | paused | WARP 已 healthy 3/3，可作海外兜底 |

### Compliance boundary (hard)

- **禁止**实现：机场/VPN **自动注册**、验证码/邮箱/手机号绕过、批量薅试用、账号工厂、ToS 规避自动化。
- **允许**：用户自行注册后提供的 **subscription URL / token** 接入；公开免费订阅列表的 preview→apply；人工批量粘贴 URL；质量门禁与验证。
- 历史任务已明确排除 “VPN account registration automation”（见 free-source-expansion / e2e-smoke 等）。

## Child Task Map

| Child | Owns | Independently verifiable deliverable |
|-------|------|--------------------------------------|
| `07-11-quality-admission-scoring` | 多目标验证、超时、延迟曲线、trend 入分、min_score 策略 | 入库更严 + best 不再是 10s 节点 |
| `07-11-subscription-xray-overseas` | 订阅供给质量 + xray 激活/验证/海外可用；衔接 `07-08-vless-xray-validation` | active xray 节点 > 0 且对海外目标可测通 |
| `07-11-trial-sub-intake-workflow` | 试用/自有订阅的**人工/半自动**接入工作流（非自动注册） | URL 录入 → preview → apply → 质量报告闭环 |
| `07-11-ops-cleanup-pool-tiers` | 低分清理、stale、分池/过滤、取用默认门槛 | free 降噪；取代理默认 L1 门槛 |

Parent 负责：源需求、子任务映射、跨子任务验收、最终集成回顾。Parent 本身不写实现代码，除非出现无 child 无法承载的集成项。

## Requirements (Parent-level)

### R1 — 海外稳定定义可测

- 必须有明确的海外验证目标集合与 SLA（延迟/成功率/最小可用节点数）。
- 验证目标矩阵见 **D1**（CF trace + ipify + YouTube）。
- 网关/选路在“海外稳定”场景下只消费 **stable 池**（见 **D4**：xray + WARP）；free 即使验证通过也只作补充候选。

### R2 — 供给优先级

1. **订阅加密节点（xray）** — 主路径（见 **D3**）
2. **WARP 链路** — 稳定兜底（仅当 xray 不可用/不足）
3. **已验证海外 free/basic** — 低优先级补充（是否进稳定池见 **D4**）
4. **未验证 free list** — 仅候选，不得成为 best 默认来源

### R3 — 准入与评分

- 新候选默认更严：超时更短、多目标可选、min_score 提高。
- 评分必须让高延迟节点显著低于低延迟节点（修复 >2s 平台期）。
- 近期 trend 应影响保留/推荐（至少影响推荐；入分由 child 设计）。

### R4 — 试用订阅接入（合规）

- 提供“订阅 URL 接入工作流”：录入、preview 推荐、apply、验证报告、失败可回滚/禁用源。
- **不包含**机场自动注册、验证码破解、批量账号创建。

### R5 — 运营可执行

- dry-run cleanup → apply cleanup 文档化并可 MCP/API 执行。
- 取代理默认支持/推荐 `min_score` + `max_latency` + `alive` + `overseas` 组合。
- free 与 subscription/xray 在观测上可区分（source / protocol / pool tier）。

### R6 — 与在途任务协同

- 不重复实现 `07-08-vless-xray-validation` 已覆盖能力；本 parent 的 xray child 以集成、缺口补齐、验收对齐为主。
- free-source-expansion 不再作为质量主路径加码。

## Out of Scope (Parent)

- 付费代理 API / 付费 VPS 采购与对接。
- 机场/VPN 自动注册与试用薅取自动化。
- Dashboard UI 大改。
- 直接 SSH / host Docker 作为默认验证手段。
- 把 free list 数量当作成功指标。

## Cross-child Acceptance Criteria

- [ ] AC1: 海外验证目标与 SLA 写死在配置/文档，并可被 validator/xray 复用。
- [ ] AC2: `service_status.quality` 中 good+excellent 有可解释提升，或存在独立“海外可用”计数（xray active / warp healthy / overseas validated）。
- [ ] AC3: `get_best_proxy` 在合理过滤下不再返回 > 用户 SLA 的慢节点作为“最优无过滤默认”的唯一故事；文档给出推荐过滤参数。
- [ ] AC4: xray `active_nodes >= N`（N 由子任务/目标 SLA 确定，建议先 N≥3），且抽样对海外目标矩阵通过。
- [ ] AC5: 试用/自有订阅可通过 intake 工作流接入，全程无自动注册代码路径。
- [ ] AC6: cleanup dry-run/apply 与分池/过滤策略可运行，stale/低分可被收敛。
- [ ] AC7: 本地 `cargo test` / `clippy -D warnings` 在相关 crate 通过；dev 验证遵循 `docs/dev-validation.md`（无默认 SSH）。

## Success Metrics (2-week aspirational)

| Metric | Baseline (approx) | Target (aligned with D2) |
|--------|-------------------|---------------------------|
| xray active | 0 | ≥ **3** stable |
| best usable overseas latency | ~11s free | p50 **≤ 2000ms** on validated path |
| recent_success_rate (validated tier) | 0.37 overall | ≥ 0.70 on retained overseas tier |
| admission timeout | pool 15s | overseas profile **5s** |
| default min_score (overseas/recommended) | 0.1 | **0.35** |
| auto-registration code paths | n/a | **0** |

## Decisions

### D1 — 海外验证目标矩阵（2026-07-11）

采用 **方案 A（三层）**，validator / xray admission / matrix 探测共用同一语义：

| Layer | URL | Purpose |
|-------|-----|---------|
| 连通性 | `https://www.cloudflare.com/cdn-cgi/trace` | 证明能出网 |
| 身份 | `https://api.ipify.org`（或等价 httpbin ip） | 观察出口 IP |
| 业务 | `https://www.youtube.com` | 证明真实海外站可达 |

- 高质量/海外稳定档：**三层全过**才算 admission 成功（具体 timeout/并发由 child 设计）。
- 现有单目标 `validate_target_url` 可保留为兼容默认；海外稳定路径显式启用多目标。

### D2 — 海外稳定档 SLA（2026-07-11，务实档）

| 参数 | 值 | 说明 |
|------|-----|------|
| 单目标超时 | **5s** | 低于当前 pool 默认 15s；与 xray 示例 5s 对齐 |
| 准入条件 | **D1 三层全过** | CF + ipify + YouTube |
| 可接受延迟 | **p50 ≤ 2000ms** | 取用/稳定池过滤；>2s 不得当 best 无过滤默认 |
| xray active 下限 | **≥ 3** | 跨子任务验收 AC4 的 N |
| 连续成功 | **≥ 2** | 进入“稳定池/稳定档”前的最小连续验证成功次数 |
| 默认 min_score | **0.35** | 高于当前 0.1；具体是否改全局默认由 scoring child 设计兼容策略 |
| 取用 max_latency | **2000ms** | 文档与 API/MCP 推荐过滤参数 |

Child 可将上述值落为配置默认或“overseas profile”，但不得静默放宽到 B/C 而未更新本 PRD。

### D3 — WARP vs xray 优先级（2026-07-11）

采用 **xray 优先，WARP 仅 fallback**：

| 条件 | 行为 |
|------|------|
| 存在满足 D1/D2 的 active xray 节点 | 海外路由优先选 xray |
| xray active < 3 或全部失败/熔断 | 回退 WARP（当前 3/3 healthy 可立刻兜底） |
| xray 恢复到 SLA | 自动回到 xray 优先 |

- 不把 WARP 与 xray 无差别混排为默认主路径（避免 WARP 带宽被爬虫式打满）。
- 并列择优可作为后续增强，不在本 parent MVP。

### D4 — 稳定池组成（2026-07-11）

**海外稳定池（stable overseas）= xray active + WARP healthy only。**

| 来源 | 可进 stable？ | 角色 |
|------|---------------|------|
| xray（订阅加密节点，通过 D1/D2） | 是 | 主路径 |
| WARP | 是 | fallback |
| free/basic HTTP（即使 overseas + 三层通过） | **否** | 仅补充候选 / extended，不得标 stable |
| 未验证 free list | 否 | 原始候选 |

- 网关“海外稳定”选路默认只消费 stable。
- free 仍可被 `get_proxy` 等显式过滤取用，但不参与 stable 语义，也不应成为海外稳定 best 的默认故事。

## Open Questions

- Parent 产品决策 **D1–D4 已闭合**。
- 无剩余阻塞 parent 规划的产品问题。
- 执行层下一步：评审 parent 产物后，为 **第一个 child** `07-11-quality-admission-scoring` 补全 `design.md` / `implement.md` 再 `task.py start`（parent 本身不 start 写码）。
- 并行：`07-08-vless-xray-validation` 继续作为 xray/VLESS 主实现轨；child `subscription-xray-overseas` 只做集成与缺口。

## Notes

- Child 实施顺序建议（非硬依赖，但推荐）：
  1. `quality-admission-scoring`（先修正度量与门禁）
  2. `subscription-xray-overseas`（主供给）
  3. `ops-cleanup-pool-tiers`（降噪）
  4. `trial-sub-intake-workflow`（扩大合规供给）
- `trial-sub-intake-workflow` 的产品边界以本 PRD 合规条款为准，设计阶段不得回退为自动注册。
- 关于“搜索机场做自动化注册拿试用流量”：已判定为 **Out of Scope / 禁止**；替代方案是人工注册后 URL 接入 + 公开订阅源 + xray 验证。
