# Business Target Routing Validation v1

## Goal

Improve real business access for targets such as OpenAI, Reddit, ChatGPT,
Discord, and X/Twitter by making routing and validation match those targets
instead of relying only on generic GeoIP and IP echo checks.

## Background

- Live gateway probes through `100.64.0.2:9080` showed GitHub and Hacker News
  can return `200` through the existing route, while `api.openai.com`,
  `chatgpt.com`, Reddit, Discord, and X either returned gateway `502` or timed
  out.
- `route_test` showed some business domains such as `openai.com`,
  `www.reddit.com`, and `x.com` can be classified as `UNKNOWN` and then routed
  as domestic/direct.
- `route_debug::geoip_plan` currently uses `GeoIPLookup::is_overseas`, and
  `GeoIPLookup::is_overseas` treats `UNKNOWN` as not overseas.
- Generic validation targets such as Cloudflare trace do not prove a proxy can
  reach OpenAI API, Reddit, or other community/business sites.

## Requirements

1. Built-in business domains must not fall back to direct routing merely because
   GeoIP returns `UNKNOWN`.
2. Route rule matches remain higher priority than built-in business-domain
   fallback, except a router `default` should not mask the built-in business
   fallback.
3. GeoIP `UNKNOWN` should be treated as overseas for gateway route selection
   while preserving existing proxy enrichment behavior.
4. Validation targets must support expected HTTP statuses so business probes can
   treat responses such as OpenAI API `401 Unauthorized` as network reachable.
5. Existing `validate_target_url` and `validate_target_urls` behavior remains
   compatible when no structured targets are configured.
6. API/MCP route diagnostics continue to expose stable route decisions.
7. No direct SSH or host Docker access is allowed for verification.

## Acceptance Criteria

- `route_test` for built-in domains such as `api.openai.com`, `chatgpt.com`,
  `www.reddit.com`, `old.reddit.com`, `oauth.reddit.com`, `discord.com`,
  `x.com`, and `twitter.com` chooses an overseas candidate plan when no explicit
  non-default route rule overrides it.
- Route diagnostics identify business fallback and GeoIP unknown fallback with
  distinct `matched_reason` values.
- Unit tests cover:
  - built-in business domain suffix matching;
  - router default not masking built-in business fallback;
  - GeoIP `UNKNOWN` routing as overseas without changing `GeoIPLookup` storage
    semantics;
  - structured validation target fallback from legacy URL fields;
  - validator success for configured expected status such as `401`.
- `config/settings.example.yaml` documents structured validation targets.
- Relevant local Rust tests and checks pass.
- Direct-reachable targets such as GitHub and Hacker News are not included in
  the built-in business overseas fallback unless future evidence shows direct
  routing is unreliable.

## Out Of Scope

- Paid provider integration.
- Full verified business pool tags and per-domain proxy selection.
- Per-domain WARP/free-pool cooldown.
- Mutating remote dev configuration through SSH.
