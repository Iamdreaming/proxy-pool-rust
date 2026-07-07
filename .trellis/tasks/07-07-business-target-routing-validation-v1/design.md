# Design: business-target-routing-validation-v1

## Routing

`proxy-core::route_debug::UpstreamSelector` remains the owner of gateway route
planning. The MVP adds a small built-in business-domain classifier for domains
that are known to be real overseas business targets:

- `openai.com`
- `api.openai.com`
- `chatgpt.com`
- `reddit.com`
- `old.reddit.com`
- `oauth.reddit.com`
- `discord.com`
- `x.com`
- `twitter.com`
Explicit non-default route rules still win. A router default match only applies
after the business-domain classifier has a chance to classify the host. This
prevents `default -> direct` from silently routing business targets direct.
GitHub and Hacker News are intentionally not in this built-in list because live
dev probes showed the existing direct route returns `200`; forcing them into the
business fallback would reduce availability when WARP/free-pool exits are bad.

For GeoIP route fallback, `UNKNOWN` becomes an overseas route decision for
gateway planning only. The stored proxy `is_overseas` enrichment remains owned
by `GeoIPLookup::is_overseas` and is not changed in this slice.

## Validation Targets

`PoolSettings` gains structured `validate_targets` while keeping existing
fields:

```yaml
pool:
  validate_target_url: "https://www.cloudflare.com/cdn-cgi/trace"
  validate_target_urls:
    - "https://www.cloudflare.com/cdn-cgi/trace"
  validate_targets:
    - url: "https://api.openai.com/v1/models"
      expected_statuses: [401]
```

Precedence:

1. If `validate_targets` is non-empty, use it.
2. Else if `validate_target_urls` is non-empty, use those URLs with default
   successful status handling.
3. Else use `validate_target_url`.

The validator keeps backward-compatible behavior when no expected statuses are
configured: statuses `< 400` pass. When expected statuses are configured, only
those statuses pass. This lets unauthenticated business probes treat `401` as
"target reached, auth missing" instead of network failure.

## Rollback

- Removing `validate_targets` from config returns validation to existing URL
  behavior.
- Reverting the route classifier returns gateway planning to GeoIP/router-only
  behavior.
