# Implement: 订阅与 xray 海外可用路径

## Slice 1 — Transport migration (HTTP/H2 → XHTTP)

**文件**：`crates/proxy-xray/src/config_gen.rs`

1. `build_stream_settings`: 在 match 之前，将 `input.network` 为 `"http"` 或 `"h2"` 映射为 `"xhttp"`
2. Trojan `network` 处理：同样映射 `"http"`/`"h2"` → `"xhttp"`
3. 新增测试：`test_http_transport_mapped_to_xhttp`，`test_build_stream_settings_maps_http_h2_to_xhttp`

**验证**：`cargo test -p proxy-xray`，`cargo clippy -p proxy-xray -- -D warnings`

## Slice 2 — Route preference (D3/D4)

**文件**：`crates/proxy-core/src/route_debug.rs`

1. `geoip_exits(true)`: `[Xray, Warp, NoProxy]`（xray 优先，无 FreePool）
2. `exits_for_known_group("warp")`: `[Warp, Xray, NoProxy]`
3. `exits_for_known_group("xray")`: `[Xray, Warp, NoProxy]`
4. 更新测试断言

**验证**：`cargo test -p proxy-core -- route_debug`，`cargo clippy -p proxy-core -- -D warnings`

## Slice 3 — 最终验证

1. `cargo test --workspace`
2. `cargo clippy --workspace -- -D warnings`
3. `cargo fmt --all -- --check`
