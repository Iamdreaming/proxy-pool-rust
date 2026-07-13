# Design: 订阅与 xray 海外可用路径

## Gap Analysis (vs 07-08-vless-xray-validation)

| Feature | 07-08 Status | Residual Gap |
|---------|-------------|-------------|
| VLESS outbound generation | Implemented (config_gen.rs) | None |
| Validate-before-active admission | Implemented (outbound_sync.rs) | None |
| Failure cooldown / retry storm prevention | Implemented | None |
| XrayStatus lifecycle registry | Implemented | None |
| Route selection guard (validation evidence) | Implemented | None |
| HTTP/H2 transport → XHTTP mapping | **Missing** | config build errors for nodes using `network: http/h2` |
| Overseas route preference (D3) | **Divergent** | `geoip_exits(true)` = [Warp, Xray, FreePool, NoProxy]; should be [Xray, Warp, NoProxy] |
| Stable overseas signal (D4) | **Missing** | No `active_nodes >= 3` health signal; now covered by PoolTier |
| Overseas target profile | Comment-only | Not enabled by default; documented in settings.example.yaml |

## F1 — Align xray admission with D1/D2

**Already done.** `XrayValidationPlan::from_settings` prefers `xray.validate_targets` and falls back to `pool.effective_validate_targets()`. The overseas profile targets (CF trace + ipify + YouTube) are documented in `config/settings.example.yaml` and `docs/score-retention.md`. Operators enable them by uncommenting the overseas profile section.

No code change needed.

## F2 — Transport migration (HTTP → XHTTP)

**Code change in `config_gen.rs`:**

- `build_stream_settings`: map `network: "http"` and `"h2"` → `"xhttp"` before building stream JSON
- Trojan `network` handling: same mapping
- xray-core 1.8.24+ removed `http` transport; `xhttp` (splithttp) is the replacement

## F3 — Stable overseas signal

**Already done via PoolTier** (implemented in ops-cleanup-pool-tiers task):
- `PoolTier::Stable` = xray active ≥ 3 + WARP healthy ≥ 1
- Exposed in `/api/status` and MCP `service_status`
- Prometheus metric `proxy_pool_tier`

No additional code change needed.

## F4 — Route preference contract

**Code change in `route_debug.rs`:**

- `geoip_exits(true)`: change from `[Warp, Xray, FreePool, NoProxy]` to `[Xray, Warp, NoProxy]`
  - D3: xray first, WARP fallback
  - D4: stable = xray + WARP only (no FreePool in overseas path)
- `exits_for_known_group("warp")`: remove FreePool → `[Warp, Xray, NoProxy]`
- `exits_for_known_group("xray")`: remove FreePool → `[Xray, Warp, NoProxy]`
- `free_pool` group: unchanged (explicit free_pool route still includes all exits)

## Compatibility

- Route order change is a **behavioral change**: overseas traffic now prefers xray over WARP, and FreePool is no longer in the automatic overseas fallback chain
- This is intentional per D3/D4 decisions
- Operators who need FreePool for overseas can use the explicit `"free_pool"` route group

## Rollback

- Revert `geoip_exits` to `[Warp, Xray, FreePool, NoProxy]` if xray is unreliable
- Transport mapping is safe: `http`/`h2` nodes would fail anyway without the mapping
