# Design: high-risk batch (B1-B5 + A1-A3)

## B1 — xray routing rules never installed (encrypted nodes egress direct)

Root cause: bootstrap `api.services` = `["HandlerService"]` only (config_gen.rs:55);
`add_xray_config` installs inbound+outbound but never the routing rule
(outbound_sync.rs:455-483). With no matching rule, xray sends each dynamic socks
inbound to the first outbound = `direct` freedom.

Fix:
1. config_gen.rs `generate_bootstrap_config`: `api.services` → `["HandlerService","RoutingService"]`.
2. config_gen.rs `generate_routing_rule_json`: add `"ruleTag": "rule-{tag}"` so the rule
   is removable.
3. xray_client.rs: add `add_routing_rule(&Value)` → `execute_cli_api("adrules", {"routing":{"rules":[rule]}})`
   with `--append` semantics, and `remove_routing_rule(rule_tag)` →
   `execute_cli_api("rmrules", ...)` (best-effort; leftover rules referencing removed
   tags are inert). `execute_cli_api` needs an optional extra arg (`--append`) — add a
   variant `execute_cli_api_with_args`.
4. outbound_sync.rs `add_xray_config`: after add_outbound, call add_routing_rule; on
   failure roll back inbound+outbound. `cleanup_xray_config` also removes the rule.
5. Test: assert bootstrap services contains RoutingService; assert routing rule carries
   ruleTag; unit-test the adrules wrapper shape.

Note: xray CLI adrules verified available (XTLS/Xray-core rules_add.go). Runtime E2E is
observed post-deploy, not in unit tests.

## B2 — stale xray cleanup leaves pool Proxy entry

outbound_sync.rs stale-removal path (~348-377) releases port + removes xray tags but
never `proxy_store.remove`. Fix: build the `127.0.0.1:{port}` socks5 dedup key and call
`proxy_store.remove` in the same block. Reuse `Proxy::new(...).dedup_key()` or the stored
XrayNode's port. Test: after marking a node stale, store no longer contains the entry.

## B3 — ProxyStore::add wipes history on re-add

store.rs `add` → `remove_existing` then insert fresh clone. Subscription refresh re-adds
basic nodes with zeroed counters. Fix: in `add`, look up existing member by dedup_key
first; if present, carry forward `success_count`, `fail_count`, `last_check`, `latency_ms`,
`quality_history`, `circuit_*`, `score`-relevant fields onto the incoming proxy unless the
incoming one already has history (incoming from validation has counts). Simplest correct
rule: if incoming.success_count==0 && incoming.fail_count==0 (i.e. a fresh
subscription/fetch entry) and an existing entry exists, preserve the existing stats fields.
Keep incoming's config/source/host/port/protocol. Test: add proxy, mark success, re-add
fresh copy → counts preserved.

## B4 — bidirectional_copy truncates on half-close

gateway http_connect.rs and socks5.rs use `select!` over two `copy` futures. Replace the
manual relay with `tokio::io::copy_bidirectional(&mut client, &mut upstream)` which handles
half-close correctly. Keep byte-count logging via its `(a_to_b, b_to_a)` return. Verify no
behavioral reliance on the select (e.g. early-abort) — if a timeout wrap exists, keep it
around copy_bidirectional.

## B5 — clash plugin-opts type mismatch drops whole subscription

clash.rs `ClashProxyEntry.plugin_opts` (and any map-valued field typed as String) must be
`Option<serde_yaml::Value>` (or `#[serde(default)] serde_json::Value`) so a map doesn't
fail deserialization of the entire document. Audit ws-opts headers values and `port`
(accept string or int via `serde_yaml::Value` / a deserialize-with). Test: clash doc with
`plugin-opts: {mode: websocket}` and quoted `port: "443"` parses the ss node.

## B6 — vmess numeric port rejected

base64_uri.rs vmess parse `get_str("port","pnt")` only reads strings. Add a numeric
fallback (mirror the `aid` handling directly above). Test: vmess link with `"port": 443`.

## B7 — percent_decode corrupts UTF-8

base64_uri.rs `percent_decode` pushes each byte as `char`. Replace body with
`percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned()` (crate already
depends on `url`; add `percent-encoding` or use `url::percent_encoding` if available) — or
collect bytes then `String::from_utf8_lossy`. Preserve existing behavior for invalid
sequences (lossy). Tests already cover ascii; add `%E4%B8%AD` → "中".

## A1 — Https proxy validated over TLS to proxy port

models.rs `url()` yields `https://host:port` for Protocol::Https; validator's
`reqwest::Proxy::all` then TLS-handshakes the proxy → always fails. Gateway upstream.rs
already treats Https like http CONNECT. Fix: give Proxy a `proxy_url()` (or adjust
validator) that emits `http://host:port` for Https so reqwest speaks plaintext to the
proxy and CONNECTs to the https target. Keep `url()` as-is if used elsewhere for display;
add `proxy_connect_url()` used by validator. Test: Https proxy → proxy url starts http://.

## A2 — strict all-targets admission (pool)

validator.rs `validate_one_against_targets` + `strict_target_admission_result` require all
targets. Add a quorum mode: alive if >=1 target passes; the returned proxy's latency/counts
reflect the passing check; failing targets recorded but not fatal. Wire pool revalidation/
fetch (scheduler.rs) to quorum; xray admission (outbound_sync validate_candidate) keeps
strict (its targets are operator business endpoints). Also drop httpbin.org from
DEFAULT_MATRIX/pool default targets, keep cloudflare trace + generate_204. Tests: 1-of-2
pass → alive under quorum, dead under strict.

## A3 — gateway selects from whole pool

route_debug.rs `try_pool_candidates` → store `get_random_candidates` weighted-random over
all. Add store method `get_top_candidates(protocol, k, limit)` that takes top-K by score
(zrevrange) then weighted-random within, and raise FREE_POOL_CANDIDATE_LIMIT 4→8. Fallback
to full-pool random if fewer than 4 pass min_score. Test: top-K selection prefers high
score.

## Ordering / risk

Independent parser fixes (B5-B7, B6) and A1 low risk → do first. B3/B2 touch store/xray
lifecycle. B1 is the highest-value and needs rollback care. A2/A3 change admission and
selection semantics — cover with unit tests. All gated by cargo test + clippy.
