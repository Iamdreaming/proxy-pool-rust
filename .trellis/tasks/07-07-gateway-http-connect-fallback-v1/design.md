# Design: gateway-http-connect-fallback-v1

## Data Flow

HTTP CONNECT path:

```text
client CONNECT host:port
  -> http_connect::handle
  -> selector.select_with_trace(host, "http")
  -> candidate Upstream::Warp / Xray / Proxy / NoProxy
  -> connect_to_upstream(candidate, target)
  -> on candidate failure: record metrics and feed WARP failure to balancer
  -> HTTP 200 tunnel or next candidate
```

SOCKS5 path:

```text
client SOCKS5 CONNECT host:port
  -> socks5::handle
  -> selector.select_with_trace(host, "socks5")
  -> candidate Upstream::Warp / Xray / Proxy / NoProxy
  -> connect_to_upstream(candidate, target)
  -> on candidate failure: record metrics and feed WARP failure to balancer
  -> SOCKS5 success reply or next candidate
```

## Implementation Shape

1. Add `connect_via_http_proxy(upstream_addr, target_addr)` in
   `crates/proxy-gateway/src/upstream.rs`.
   - Open TCP to the upstream HTTP proxy.
   - Send `CONNECT target_addr HTTP/1.1`.
   - Require a `2xx` status line.
   - Consume headers until `\r\n\r\n`.
   - Return the connected stream.
2. Update `connect_to_upstream` to dispatch `Upstream::Proxy(proxy)` by
   `proxy.protocol`:
   - `Protocol::Http` and `Protocol::Https` -> HTTP CONNECT upstream.
   - `Protocol::Socks5` -> SOCKS5 upstream.
   - `Protocol::Socks4` -> unsupported error for now.
3. Add per-candidate timeout in both gateway handlers around
   `connect_to_upstream`.
   - Keep timeout local to gateway attempts.
   - Use a conservative constant initially, e.g. 5 seconds.
   - On timeout, record the attempt as failure and continue to the next
     candidate.
4. Expand `free_pool` into a small bounded set of concrete proxy candidates.
   - Keep the high-level route order unchanged.
   - Keep metrics labeled by `exit=free_pool`.
   - Use weighted random selection without replacement so repeated attempts are
     distinct proxies and still prefer higher-scored entries.
5. Preserve the WARP instance id in `Upstream::Warp`.
   - `WarpBalancer::next()` already returns the concrete instance.
   - The selected `Upstream::Warp` should carry both `id` and `socks5_port`.
6. Add gateway attempt feedback on concrete failure.
   - HTTP CONNECT and SOCKS5 handlers record the failed attempt through
     `UpstreamSelector`.
   - For `Upstream::Warp`, the selector calls `WarpBalancer::mark_failed(id)`.
   - The regular health checker remains responsible for marking WARP healthy
     again.
7. Keep route ordering unchanged. This task fixes connection mechanics,
   fallback progression, and minimal runtime WARP failure feedback only.

## Testing

Unit/integration-style async tests inside `proxy-gateway` should avoid live
network dependencies:

- Fake HTTP proxy server that accepts CONNECT and asserts request line.
- Fake SOCKS5 proxy server that asserts SOCKS5 greeting/request.
- Fake slow upstream candidate for timeout/fallback behavior if practical at
  handler level; otherwise unit-test timeout wrapper and rely on existing
  metrics/fallback handler tests.
- Pure core tests for weighted random multi-candidate selection without
  replacement.
- Pure core test for `WarpBalancer::mark_failed` removing the failed instance
  from healthy rotation.

## Trade-Offs

HTTP CONNECT upstream support fixes HTTP pool proxy compatibility. Gateway
failure feedback is intentionally limited to in-process WARP availability; it
does not change WARP endpoint optimization, health-check URLs, or persistent
quality scoring.
