# Design

## Data Flow

```text
subscription feed
  -> proxy-sub parsers
  -> SubscriptionProxy::Vless
  -> pending encrypted store, label "vless"
  -> proxy-xray outbound sync
  -> local xray outbound + SOCKS port
  -> proxy-core validator against configured targets
  -> proxy store only if validation passes
  -> overseas route selector
```

The key behavior change is that xray "active" becomes a validated routing
state. Adding a config entry to xray is only an intermediate step.

## Subscription Model

Add a `Vless` variant to `SubscriptionProxy` with fields aligned to common
subscription encodings:

- `name`
- `host`
- `port`
- `uuid`
- `encryption`, defaulting to `none`
- `flow`
- `network`, defaulting to `tcp`
- `security`, such as `tls` or `reality`
- `sni`
- `host_header`
- `path`
- `service_name`
- `fingerprint`
- `public_key`
- `short_id`
- `spider_x`

Parser conventions stay local to `proxy-sub`: unsupported or malformed entries
do not crash ingestion. Recognizable VLESS entries with missing critical fields
are skipped or downgraded according to the existing parser's pattern.

## Parser Coverage

- Base64 URI: parse `vless://uuid@host:port?...#name`.
- V2Ray JSON: parse `protocol: "vless"` outbound settings and stream settings.
- Clash YAML: parse `type: vless`, including `ws-opts`, `grpc-opts`, `reality-opts`,
  `tls`, `servername`, and `client-fingerprint` when present.
- Surge: parse common `vless` lines with key/value options where the existing
  parser already recognizes the type.

## Xray Outbound Generation

Generate a VLESS outbound in the shape documented by Xray:

- `protocol: "vless"`
- `settings.vnext[].address`
- `settings.vnext[].port`
- `settings.vnext[].users[].id`
- `settings.vnext[].users[].encryption`
- optional `flow`
- shared `streamSettings` for TCP, WS, gRPC, TLS, and Reality

Existing VMess/Trojan stream-setting helpers should be reused or generalized
instead of duplicating transport logic.

## Validation Filtering

`proxy-xray::OutboundSync` should receive a small validation plan from server
startup:

- validation targets: xray-specific targets if configured, otherwise pool
  validation targets
- timeout: xray-specific timeout if configured, otherwise pool timeout
- attempts per sync cycle: bounded to avoid a long validation storm
- failed-node cooldown: skips recently failed nodes so later pending nodes can
  be tried

Activation algorithm:

1. Pick pending encrypted nodes for labels `ss`, `vmess`, `trojan`, and `vless`.
2. Skip already active nodes and nodes inside the validation-failure cooldown.
3. Allocate a local port and add the xray outbound.
4. Build a temporary local SOCKS proxy record for `127.0.0.1:{port}`.
5. Validate that local proxy against the selected targets.
6. On success, store the validated proxy with quality metadata and mark the node
   active.
7. On failure, remove the xray outbound, release the port, mark the node failed,
   and record a retry cooldown.

This keeps routeable storage restricted to validated xray endpoints.

## Route Selection Guard

The overseas xray route should ignore xray proxies that have no validation
evidence, such as legacy entries created before this feature. A runtime xray
failure should place that local endpoint in a short cooldown, matching the
existing free-pool cooldown behavior.

## Config Shape

Add optional settings under `xray` only where needed:

```toml
[xray]
validate_timeout_sec = 5
validation_attempt_limit_per_cycle = 50
validation_failure_cooldown_sec = 3600

[[xray.validate_targets]]
url = "https://www.youtube.com/generate_204"
expected_status = 204
```

If `xray.validate_targets` is empty, the system falls back to
`pool.effective_validate_targets()`. This keeps existing deployments working
while letting dev use a YouTube-focused target profile.

## Risks

- Public free VLESS nodes are often dead or region-blocked; validation may find
  few or zero usable nodes.
- Reality transport fields vary by subscription source. The first pass should
  support common field names and log unsupported combinations clearly.
- Validating hundreds of nodes can be slow. The bounded attempts and failed-node
  cooldown are required for operational safety.
- Existing unvalidated xray entries in storage must be skipped, or they can
  mask the new filtering behavior until old state expires.
