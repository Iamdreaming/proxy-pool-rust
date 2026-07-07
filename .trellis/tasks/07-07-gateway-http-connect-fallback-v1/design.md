# Design: gateway-http-connect-fallback-v1

## Data Flow

HTTP CONNECT path:

```text
client CONNECT host:port
  -> http_connect::handle
  -> selector.select_with_trace(host, "http")
  -> candidate Upstream::Warp / Xray / Proxy / NoProxy
  -> connect_to_upstream(candidate, target)
  -> HTTP 200 tunnel or next candidate
```

SOCKS5 path:

```text
client SOCKS5 CONNECT host:port
  -> socks5::handle
  -> selector.select_with_trace(host, "socks5")
  -> candidate Upstream::Warp / Xray / Proxy / NoProxy
  -> connect_to_upstream(candidate, target)
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
   - Use a conservative constant initially, e.g. 8 seconds.
   - On timeout, record the attempt as failure and continue to the next
     candidate.
4. Keep route ordering unchanged. This task fixes connection mechanics and
   fallback progression only.

## Testing

Unit/integration-style async tests inside `proxy-gateway` should avoid live
network dependencies:

- Fake HTTP proxy server that accepts CONNECT and asserts request line.
- Fake SOCKS5 proxy server that asserts SOCKS5 greeting/request.
- Fake slow upstream candidate for timeout/fallback behavior if practical at
  handler level; otherwise unit-test timeout wrapper and rely on existing
  metrics/fallback handler tests.

## Trade-Offs

HTTP CONNECT upstream support is the minimal correct fix for HTTP pool proxies.
It does not solve WARP health lying about real target reachability. That should
be a follow-up task because it may require feedback from gateway attempt
failures into the WARP balancer/health model.
