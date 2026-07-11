# Implement: authenticated proxy support

Validate per crate: `cargo test -p <crate>`; final `cargo test` + clippy `-D warnings`.

1. [x] proxy-core models: add `username`/`password` (serde default) to `Proxy`;
       init None in `new`; add `credentials()` helper. Update `proxy_connect_url`
       usage (validator).
2. [x] proxy-sub models: add `username`/`password` to `SubscriptionProxy::Basic`.
3. [x] Parsers (compile-fix all `Basic {..}` constructions + extract creds):
       clash.rs (add `username` field + http/socks5 arms), base64_uri.rs
       (parse_basic userinfo), surge.rs + v2ray_json.rs (None to compile).
4. [x] convert.rs `to_proxy`: carry username/password into Proxy.
5. [x] validator.rs: `reqwest::Proxy::all(...).basic_auth(u,p)` when creds present.
6. [x] gateway upstream.rs: creds param on connect_via_http_proxy (Proxy-Auth
       header) + connect_via_socks5/handshake (RFC1929 0x02); callers pass creds
       for Upstream::Proxy, None for WARP/xray/chain-warp-hop. Add base64 dep.
7. [x] store.rs carry_forward_history: comment + test fresh-creds preserved.
8. [x] Tests: clash http auth, base64 userinfo, validator auth build, gateway
       CONNECT Proxy-Authorization bytes, socks5 auth framing, carry-forward creds.
9. [x] Full test + clippy; trellis-check; commit; push; CI.
10. [ ] Config: append URL to subscription.urls (PUT /api/settings) + restart
        proxy-pool; verify source parses (logs) and http/https pool rises.

## Rollback
Additive to model/parsers/gateway; revert commit to remove. Config change is
separate (remove URL from settings).
