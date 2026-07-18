# VLESS support and xray validation filtering

## Summary

Add first-class VLESS subscription support and make xray activation mean "usable
for a configured validation target", not merely "accepted by the local xray
API". The product goal is to turn GitHub airport subscription feeds into a
smaller, higher-confidence xray candidate set that can be routed for overseas
sites such as YouTube.

## Background

Current subscription ingestion recognizes several encrypted node types, but
VLESS is treated as unknown in the URI, V2Ray JSON, Surge, and Clash parsing
paths. Because unknown nodes are intentionally excluded from pending encrypted
storage, VLESS nodes never reach xray activation.

Current xray activation also treats local xray config creation as success. A
node can therefore be marked active and selected by the overseas route even if
it cannot reach the desired external site. Recent live sampling showed this is
the wrong success definition for the user's goal: many active xray nodes were
not usable for YouTube.

## Goals

- Parse common VLESS subscription formats into a supported encrypted node
  variant instead of `Unknown`.
- Store VLESS nodes in the pending encrypted queue and include them in xray
  synchronization.
- Generate xray outbound configuration for common VLESS transports.
- Validate xray candidates through the local SOCKS port before making them
  routeable.
- Keep route selection from repeatedly choosing xray nodes that were never
  validated or that have just failed at runtime.

## Non-Goals

- Buying, provisioning, or guaranteeing premium upstream nodes.
- Supporting every VLESS extension in the wild on the first pass.
- Adding support for unrelated protocols such as Hysteria, TUIC, WireGuard, or
  sing-box-only formats.
- Changing the dev deployment boundary: no SSH or host Docker validation.

## Functional Requirements

1. `proxy-sub` has a VLESS model variant with host, port, uuid, encryption,
   flow, transport, TLS/Reality, SNI, host header, path, fingerprint, public key,
   short id, and service-name style fields where available.
2. Base64 URI, V2Ray JSON, Clash YAML, and Surge parsers map recognizable VLESS
   entries to that variant. Malformed VLESS entries are logged and skipped or
   mapped to `Unknown` according to the existing parser convention.
3. VLESS nodes participate in `protocol_label`, host/port extraction, deduping,
   protocol counts, `encrypted_nodes`, and pending encrypted Redis storage.
4. `proxy-xray` can generate VLESS outbound JSON for at least TCP, WS, gRPC,
   TLS, and Reality-style inputs when required fields are present.
5. xray synchronization validates a candidate's local SOCKS endpoint against
   configured validation targets before storing it as active in the proxy pool.
6. Failed validation cleans up the temporary xray outbound and local port, marks
   the encrypted node as failed with a readable reason, and avoids immediate
   retry storms.
7. Overseas route selection ignores legacy/unvalidated xray entries and applies
   a short cooldown when an xray upstream fails during a request.
8. Status and source reports continue to distinguish parsed, stored encrypted,
   active, failed, and unknown nodes.

## Acceptance Criteria

- Unit tests cover VLESS parsing for URI and at least one structured feed format.
- Pending storage round-trips a VLESS node under the `vless` protocol label.
- xray config-generation tests cover VLESS outbound JSON for a TLS/WS path and
  a Reality path when the fixture contains Reality fields.
- Outbound synchronization tests prove that a validation failure does not leave
  the proxy in the routeable pool and that a validation success stores quality
  metadata.
- Route selection tests or focused unit coverage prove unvalidated xray proxies
  are skipped.
- `cargo test` is run for the touched crates; broader workspace testing is run
  if time and dependency state allow it.
- Dev validation, if pushed and deployed, follows `docs/dev-validation.md`:
  GitHub Actions image first, public `/api/status` and `/api/readyz`, then MCP
  read-only status; service mutation only with explicit operator/user choice.

## Product Decision

Validation targets are configurable. By default, xray admission validation
inherits the pool validation targets. For the YouTube-oriented dev rollout, use
an xray-specific profile with YouTube `generate_204`, Google `generate_204`, and
Cloudflare trace.
