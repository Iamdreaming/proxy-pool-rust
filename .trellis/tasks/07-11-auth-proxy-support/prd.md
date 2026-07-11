# PRD: Authenticated HTTP/SOCKS5 proxy support

## Goal

Support upstream proxies that require username/password auth, so subscription
sources of authenticated HTTP/SOCKS5 proxies (e.g. TopChina/proxy-list
clash_sub.yaml — 266 auth'd HTTP proxies, usernames rotate hourly) become
usable. Today the Clash parser drops the credentials, the `Proxy` model has no
auth fields, and the validator/gateway never send auth, so such proxies fail
validation (407) and are useless.

## Requirements

R1. `SubscriptionProxy::Basic` and `Proxy` carry optional `username`/`password`.
    `Proxy` fields use `#[serde(default)]` for backward-compatible stored JSON.
R2. Parsers extract credentials:
    - Clash: `type: http`/`socks5` → Basic with `username`/`password`.
    - base64 URI: `scheme://user:pass@host:port` userinfo (percent-decoded).
R3. convert `to_proxy` carries credentials into the pool `Proxy`.
R4. Validator authenticates: builds the reqwest proxy with basic auth when creds present.
R5. Gateway authenticates upstream:
    - HTTP CONNECT: send `Proxy-Authorization: Basic base64(user:pass)`.
    - SOCKS5: negotiate username/password auth (method 0x02) when creds present;
      keep no-auth (0x00) otherwise. WARP/xray/chain (local ports) stay unauth.
R6. Credential freshness: hourly refresh applies NEW credentials.
    `carry_forward_history` must NOT carry old username/password.
R7. Add the subscription URL to config; deploy.

## Constraints

- Backward compatible: stored `Proxy` JSON without auth still deserializes.
- Don't leak credentials in error messages/logs.

## Out of scope

- Auth for encrypted (ss/vmess/trojan/vless) nodes.
- Surge/v2ray-json auth parsing unless trivial.

## Acceptance criteria

1. `cargo test` green; clippy `-D warnings` clean.
2. Clash test: `type: http` + username/password → Basic carries both.
3. base64 test: `http://user:pass@host:port` → Basic with decoded creds.
4. Validator builds authenticated reqwest proxy when creds present (unit test).
5. Gateway `connect_via_http_proxy` emits `Proxy-Authorization` when creds given.
6. `carry_forward_history` keeps incoming (fresh) creds.
7. Post-deploy: source's HTTP proxies validate + enter pool (count rises).
