# Research: Xray VLESS and Validation Filtering

## Local Code Evidence

- `crates/proxy-sub/src/parser/base64_uri.rs` recognizes `vless://` as a URI
  prefix but currently maps it to `Unknown`.
- `crates/proxy-sub/src/parser/v2ray_json.rs` maps `protocol: "vless"` outbounds
  to `Unknown`.
- `crates/proxy-sub/src/parser/clash.rs` handles socks5, http, ss, vmess, and
  trojan but not `type: vless`.
- `crates/proxy-sub/src/parser/surge.rs` lists `vless` as a known type but does
  not parse it into a supported encrypted variant.
- `crates/proxy-sub/src/convert.rs` only treats Shadowsocks, VMess, and Trojan
  as encrypted pending nodes.
- `crates/proxy-xray/src/outbound_sync.rs` only syncs labels `ss`, `vmess`, and
  `trojan`; it marks nodes active after xray config creation, before any target
  reachability validation.
- `crates/proxy-xray/src/config_gen.rs` emits xray outbound JSON for
  Shadowsocks, VMess, and Trojan only.
- `crates/proxy-core/src/validator.rs` already supports validating one proxy
  against multiple structured targets, so xray filtering can reuse existing
  validation primitives.
- `crates/proxy-core/src/route_debug.rs` selects active xray nodes by encrypted
  state, which can include legacy/unvalidated entries.

## External Docs

- Xray outbound config object:
  <https://xtls.github.io/en/config/outbound.html>
- Xray VLESS outbound settings:
  <https://xtls.github.io/en/config/outbounds/vless.html>

The current VLESS outbound shape uses `protocol: "vless"` and direct
`settings.address`, `settings.port`, `settings.id`, and `settings.encryption`
fields, with optional `flow`. Transport details live under the shared
`streamSettings` object.

## Product Evidence

Previous live checks showed the dev service had many active xray nodes, but a
sample against YouTube `generate_204` produced no successful candidates. That
means the current active count is not a usable proxy-quality signal for the
YouTube goal.
