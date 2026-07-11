# Design: authenticated proxy support

## Model changes

`SubscriptionProxy::Basic { host, port, protocol, username: Option<String>,
password: Option<String> }` (proxy-sub/models.rs).

`Proxy` (proxy-core/models.rs): add
```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub username: Option<String>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub password: Option<String>,
```
Set in `Proxy::new`? No — keep `new` unchanged (creds set by convert). Add both
to the struct initializer in `new` as `None`.

## Parsers

- clash.rs: add `username: Option<String>` to `ClashProxyEntry` (password field
  exists). http/socks5 arms → `Basic { .., username: entry.username.clone(),
  password: non_empty(entry.password) }`. Empty string → None.
- base64_uri.rs `parse_basic(rest, protocol)`: split optional `user:pass@` before
  host:port; percent-decode; pass into Basic. Update `SubscriptionProxy::Basic`
  construction there and in surge.rs/v2ray_json.rs (which also build Basic) to
  include `username: None, password: None` (compile fix), extracting where easy.

## convert.rs

`to_proxy`: after `Proxy::new`, set `proxy.username`/`proxy.password` from the
Basic fields.

## Validator (proxy-core/validator.rs)

`check_one` builds `reqwest::Proxy::all(proxy.proxy_connect_url())`. Change to:
```rust
let mut rp = reqwest::Proxy::all(proxy.proxy_connect_url())?;
if let (Some(u), Some(p)) = (&proxy.username, &proxy.password) {
    rp = rp.basic_auth(u, p);
}
```
(`basic_auth` avoids URL-encoding the rotating base64-ish username.)

## Gateway (proxy-gateway/upstream.rs)

- `connect_via_http_proxy(upstream_addr, target, creds: Option<(&str,&str)>)`:
  when `creds`, add header `Proxy-Authorization: Basic <base64(user:pass)>` to
  the CONNECT request.
- `connect_via_socks5(upstream_addr, target, creds)` +
  `socks5_handshake_on_stream(stream, target, creds)`: offer method 0x02 when
  creds present; on server-selected 0x02, send the username/password auth
  sub-negotiation (RFC 1929) and check the 0x00 success reply; keep 0x00 path
  otherwise.
- Callers: `Upstream::Proxy(proxy)` arms pass `proxy.credentials()`
  (a helper returning `Option<(&str,&str)>` when both set); WARP/xray/chain pass
  `None`. `connect_via_warp_chain`'s inner socks5 to the pool proxy passes the
  proxy's creds; the WARP hop passes None.
- Add `base64` for the header. Use `base64::engine::general_purpose::STANDARD`
  (already a workspace dep via proxy-sub; add to proxy-gateway Cargo if missing).

## carry_forward_history (proxy-core/store.rs)

No auth fields in the carried list — `merged` starts as `incoming.clone()`, so
fresh credentials are kept automatically. Add a comment; add a test asserting
re-add with new creds keeps the new ones.

## Config

Add to `subscription.urls` on the server via `PUT /api/settings` (append the new
URL to the existing list), then restart proxy-pool to reload (or confirm reload
path). URL:
`https://gh-proxy.com/https://raw.githubusercontent.com/TopChina/proxy-list/refs/heads/main/clash_sub.yaml`

## Risks

- SOCKS5 auth sub-negotiation must exactly follow RFC 1929 (ver 0x01, ulen,
  uname, plen, passwd; reply ver 0x01 status 0x00). Unit-test the byte framing.
- Backward compat: serde default on new `Proxy` fields; verify old JSON loads.
- Credential leakage: never format creds into error strings.
