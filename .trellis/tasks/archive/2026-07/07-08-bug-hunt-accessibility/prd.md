# PRD: Bug hunt fixes + proxy accessibility improvements

## Background

Production signals (2026-07-08, dev 100.64.0.2): recent_success_rate 37%, top failure
reason validation_failed (18616), score buckets 3896 poor / 1535 fair / 57 good /
0 excellent, 757 stale, https pool = 0, xray 473 active / 30 failed.

Three review agents swept proxy-core/proxy-xray, proxy-gateway/proxy-sub/proxy-api,
and the accessibility pipeline. This task fixes confirmed high/medium bugs and the
highest-impact accessibility bottlenecks.

## In scope

### Correctness bugs

- B1 (critical): xray routing rule generated (`config_gen.rs` `routing_rule_json`) but
  never installed — dynamic inbounds route to the first outbound (`direct`), so
  "activated" encrypted nodes egress directly. Must verify then fix.
- B2 (high): xray stale-node cleanup releases port and xray tags but never removes the
  pool `Proxy` entry → reused port routes through wrong node.
- B3 (high): `ProxyStore::add` replaces existing entry wholesale; hourly subscription
  refresh resets success/fail counters, quality history, and circuit state. Preserve
  validation history when re-adding an existing proxy.
- B4 (high): gateway `bidirectional_copy` drops the other direction on first EOF —
  truncated responses on client half-close. Use `tokio::io::copy_bidirectional`.
- B5 (high): clash parser `plugin-opts` typed `Option<String>` but real Clash emits a
  map → whole YAML document fails, subscription yields zero nodes. Accept arbitrary value.
- B6 (med): base64_uri vmess parser rejects numeric `"port": 443`. Accept string+number.
- B7 (med): `percent_decode` maps bytes to chars → corrupts multi-byte UTF-8 credentials.
- B8 (med, latent): `PendingStore::get_pending(limit=0)` returns ALL members.
- B9 (med): scheduler revalidate skips Socks4 → dead socks4 entries never evicted.

### Accessibility improvements

- A1: `Protocol::Https` validation connects to the proxy itself over TLS → 100% fail,
  https pool = 0. Validate Https proxies with `http://` scheme, matching gateway.
- A2: strict all-targets-must-pass admission + flaky httpbin default target multiply
  down pass rate. Change pool admission to quorum (>=1 target pass = alive; failures
  only lower score); keep strict mode for xray admission.
- A3: gateway pool selection is score-weighted random over the whole pool; 3896 poor
  drown 57 good. Select from top-K by score (K=50) with weighted random inside,
  fallback to whole pool when top-K < 4 usable.
- A4: single global ConnectionPacer at 10/s starves validation. Raise default
  pace_rate_per_sec to 50 and give fetch and revalidate loops independent pacers.
- A5: score formula `(success-fail)/total` makes excellent unreachable and keeps dead
  proxies alive via stale latency. Use recent quality_history success rate (fallback
  success/total), zero latency component on last-failed entries.
- A6: gateway attempt outcomes only touch in-memory cooldown; feed Success/Failure back
  into store so real traffic shapes scores and circuit state.

## Out of scope (follow-up)

store.rs non-atomic ZSET race + O(N) writes; xray_client temp-file race & write-lock hang;
pending ZSet unbounded growth; unbounded cooldown maps; http_connect single-read parsing;
handshake/idle timeouts; IPv6 host extraction & delete_proxy IPv6 key; v2ray detect()
greediness; ss legacy base64 form; dedup_key credentials; unused per-source intervals;
GeoIP path check; circuit alive-filter inconsistency; process kill zombie.

## Acceptance criteria

1. `cargo test` zero failures; `cargo clippy --workspace --all-targets -- -D warnings` clean.
2. B1: xray config binds in-{tag} -> out-{tag} (test on pushed config / routing install path).
3. B3: re-adding existing proxy preserves counts, history, circuit state (unit test).
4. B4: gateway relays use copy_bidirectional.
5. B5/B6/B7: parser unit tests (clash map plugin-opts, numeric vmess port, UTF-8 decode).
6. A1: Https proxy validation builds an http:// proxy URL (unit test).
7. A2: quorum admission unit test.
8. A3: top-K selection unit test.
9. A5: score unit tests updated; 70% success-rate proxy lands in fair/good.
10. Deployed via standard workflow; post-deploy read-only checks show https bucket > 0
    and recent_success_rate trending up.
