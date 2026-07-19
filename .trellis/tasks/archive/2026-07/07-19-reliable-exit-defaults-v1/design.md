# Design: reliable-exit-defaults-v1

## 1. Scope

| 做 | 不做 |
|----|------|
| 重写 `config/routes.example.yaml` 主默认 | 改 QualityTier 出口表定义 |
| `settings.example.yaml` 增加 `routes_path` | 自动迁移用户 routes |
| README 路由决策链对齐 | 启服 / 部署 |
| **default 命中 + GeoIP 分流**（`route_debug` 最小改） | P0-B/C、非 default 规则插 GeoIP |
| 单测锁定 example + GeoIP default 行为 | Resume stash |
| ROADMAP Now/Done 交接 | |

## 2. Target routes.example shape

```yaml
# Primary profile: overseas-stable (Availability-First)
# default → premium (Xray → Warp → NoProxy)
# direct  → *.cn only (and other explicit domestic hosts)
# free_pool → optional dirty-ok hosts only (tier: any)

groups:
  direct:
    domains:
      - "*.cn"
      # add domestic CDN hosts as needed; do NOT put default here for overseas-stable

  overseas:
    tier: premium
    domains:
      - default

  free_pool:
    tier: any
    domains:
      - "github.com"
      # optional examples...

  # warp: optional explicit premium hosts if desired
  # openai: tier premium + domains — documented in comments

# Commented alternate: domestic-friendly
# groups:
#   direct:
#     domains: ["*.cn", "default"]
#   ...
```

### 2.1 Why premium for default

- 对齐用户目标「持续正常用代理」与 ROADMAP L1 主供给。
- premium **硬禁止** free_pool，避免未匹配流量掉进脏 IP。
- 风险：无 xray 且 WARP 不健康 → NoProxy/502。**必须在 example 头注释 + settings 旁注**说明依赖 L1 部署；这是诚实失败，优于假成功直连。

### 2.2 Default match + GeoIP（用户确认：中国→Direct）

现状：`route_match_plan` 在 `is_default` 时只做 domain helpers → **组策略**，**不**调 GeoIP。因此 `default→premium` 会把解析到国内 IP 的主机也送进代理。

目标行为：

```text
有 Router 且 is_default:
  1. direct_reachable / business domain helpers（不变）
  2. 若 default 组 direct-only → Direct（domestic-friendly，不跑 GeoIP 改写）
  3. 若 GeoIP 可用:
       国内 → Direct
       境外/UNKNOWN → default 组 tier/exits（overseas-stable = premium）
  4. 否则 → resolve_group_policy(default 组)
非 default 规则:
  仅组策略（不变）
```

实现落点：优先在 `UpstreamSelector::build_plan` 展开 default 分支（因 GeoIP 是 async + selector 字段）；`route_match_plan` 可拆成 non-default / default-without-geoip 辅助，避免双源逻辑。

### 2.2 standard 备选（未选为默认）

若未来希望 default 在 xray/warp 失败后仍可借 free_pool，可改 `tier: standard`。本任务 D1 固定 premium；PRD Open Question 允许用户改判。

## 3. settings.example

在合适位置（gateway/api 附近或文件末配置段）增加：

```yaml
# routes_path: "config/routes.yaml"
# Copy from config/routes.example.yaml. Primary example is overseas-stable:
# unmatched hosts use tier premium (Xray → Warp). Requires healthy WARP and/or
# xray nodes; otherwise gateway may return 502 instead of leaking via direct.
```

不把 `routes_path` 设成强制非空默认（避免无文件时启动失败）；**注释示例**即可满足 R2。

## 4. README 路由段目标文案（要点）

```text
1. routes.yaml 最长后缀匹配 → 组 tier / exits
   - premium: Xray → Warp → NoProxy
   - standard: Xray → Warp → FreePool → NoProxy
   - any: FreePool → Warp → Xray → NoProxy
   - direct 组: Direct only
2. 无 routes 时：GeoIP / 硬编码业务域 / any 回退
3. 示例默认（overseas-stable）：未匹配 → premium；*.cn → direct
```

删除或改写「回退链 → 池代理 → WARP → xray」作为**通用**描述。

## 5. Tests

### 5.1 Example 文件语义（`router` 或 `route_debug`）

```rust
let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/routes.example.yaml");
let text = std::fs::read_to_string(path).unwrap();
let router = Router::from_yaml_str(&text).unwrap();
assert_ne!(router.match_group("unknown.example"), "direct");
assert_eq!(router.tier_for(&router.match_group("unknown.example")), Some(QualityTier::Premium));
assert_eq!(router.match_group("foo.cn"), "direct");
```

### 5.2 Default + GeoIP（`route_debug`，可用 mock/stub GeoIP 或现有测试夹具）

- default∈premium 组 + geoip 国内 → selected exits Direct / matched_reason 含 geoip_domestic
- 同上 + geoip 境外 → premium 出口顺序
- default∈direct + geoip 境外 → 仍 Direct（不被改写）

优先复用现有 `UpstreamSelector` 测试构造方式；若 GeoIP 难 mock，抽 `geoip_exits`/`route_default_plan` 纯函数单测 + 集成级 selector 测一条。

## 6. Compatibility

| 角色 | 影响 |
|------|------|
| 新部署复制 example | 未匹配走 premium，行为**相对旧 example 变化**（有意） |
| 已有自建 routes.yaml | 无影响（不迁移） |
| 无 routes_path | 行为不变（代码路径未改） |
| 单元测试依赖旧 example 字符串 | 仅新测读取文件；旧测用内嵌 YAML 不受影响 |

## 7. Rollback

- 还原 `routes.example.yaml` / README / settings.example / 单测即可。
- 无数据迁移、无 Redis schema 变更。

## 8. Risks

| 风险 | 缓解 |
|------|------|
| 用户复制 example 后无 xray/WARP → 502 | 注释 + README 明确 L1 依赖；status `pool.tier` 信号 |
| example 注释过长难读 | 头注释 ≤30 行；细节链到 scenario-quality-tiers spec |
| include 路径在 Windows CI 失败 | `CARGO_MANIFEST_DIR` + `join` |
| 与「国内友好」用户冲突 | domestic-friendly 注释块保留 |

## 9. Files to touch

- `config/routes.example.yaml`（主）
- `config/settings.example.yaml`
- `README.md`（路由节）
- `docs/ROADMAP.md`（Now/Done/Ready 勾选）
- `crates/proxy-core/src/route_debug.rs` — default+GeoIP 行为 + 测试
- 可选：`router.rs` 测试读 example 文件
- 可选：`docs/proxy-usage.md` 一句交叉引用
- 任务目录规划产物
