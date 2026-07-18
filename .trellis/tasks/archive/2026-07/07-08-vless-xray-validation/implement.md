# Implementation Plan

## 1. Add VLESS to proxy-sub

- Add `SubscriptionProxy::Vless` and a dedicated struct or inline fields matching
  the existing model style.
- Update helper methods: `host`, `port`, `protocol_label`, dedup key handling,
  and any exhaustive match arms.
- Include VLESS in encrypted partitioning and pending-store serialization.
- Add parser coverage:
  - base64 URI
  - V2Ray JSON
  - Clash YAML
  - Surge common key/value line
- Add tests for parsing and pending round-trip behavior.

## 2. Generate VLESS xray config

- Update `proxy-xray::config_gen` to handle the VLESS variant.
- Reuse/generalize stream settings for TLS, Reality, WS, and gRPC.
- Add focused JSON-shape tests for representative VLESS fixtures.

## 3. Validate before routeable activation

- Add a validation-plan struct in `proxy-xray` and build it from app settings in
  the server wiring.
- Extend `XraySettings` with optional xray validation targets, timeout, attempt
  limit, and failure cooldown.
- Update outbound sync labels and activation guard to include `vless`.
- Validate each candidate local SOCKS endpoint before storing it in `ProxyStore`.
- On validation failure, remove xray config, release the port, mark failed, and
  add retry cooldown.
- Store successful validation metadata so route selection can identify validated
  xray proxies.

## 4. Protect route selection

- Filter xray route candidates to require validation evidence.
- Add runtime cooldown for failed xray upstream attempts.
- Keep existing Warp and free-pool behavior unchanged.

## 5. Verify locally

- Run targeted tests for `proxy-sub`, `proxy-xray`, and routing/core changes.
- Run wider `cargo test` where dependency state permits.
- Check formatting.

## 6. Optional dev rollout

Only after local verification and user approval to push:

- Commit locally.
- Push branch.
- Wait for GitHub Actions Docker image build.
- Check public `/api/status`, `/api/readyz`, MCP `service_status`, and MCP
  `update_status`.
- Confirm runtime `git_hash` matches the pushed short SHA.
- Mutating `update_service` is only used after explicit operator/user choice.
