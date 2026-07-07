# Source quality expansion v1

## Goal

Increase the number of business-usable overseas proxy exits by making source
quality measurable and making pool admission validation closer to real gateway
traffic.

The goal is not simply to store more public proxies. The gateway already shows
that free-pool availability is the bottleneck after WARP failures are cooled
down. This task should improve the quality of proxies admitted to the pool and
give operators enough source-level evidence to decide which fetchers or
subscription sources are worth expanding.

## Background And Evidence

- The deployed gateway CONNECT fix is live on dev. WARP failures are now
  bounded to the configured WARP instances and then cooled down.
- Remaining overseas instability is concentrated in `free_pool`.
- Recent dev quality status showed many stored proxies but very few strong
  entries: about 20 `good`, 0 `excellent`, and recent success rate around 43%.
- Current built-in fetchers are:
  - `proxyscrape:http`
  - `proxyscrape:https`
  - `proxyscrape:socks5`
  - `thespeedx:http`
  - `thespeedx:socks5`
  - `free_proxy_list`
  - `clarketm:http`
  - `geonode`
- Current scheduler flow in `crates/proxy-core/src/scheduler.rs` merges all
  fetcher output, deduplicates it, validates with a single
  `pool.validate_target_url`, then stores all working proxies.
- Fetcher status already reports `fetched` and `parsed`, and source-level
  circuit breaker state exists, but reports do not show how many proxies from
  each source survived validation or storage.
- `proxy-core::validator` already has multi-target matrix primitives for
  API/MCP checks, but scheduler admission still uses one validation target.

## Requirements

### R1: Business validation target list

Add a backward-compatible pool setting for multiple admission validation
targets.

- Existing `pool.validate_target_url` remains valid.
- New config should allow an operator to set multiple targets.
- When the new list is empty or omitted, behavior must remain compatible with
  the existing single-target setting.
- Validation targets should be HTTP(S) URLs.

### R2: Strict MVP admission policy

For the first implementation slice, a proxy should be admitted only when it
passes all configured admission targets.

- This is intentionally stricter than the current single-target admission.
- The default config should still behave as it does today because the target
  list is absent by default.
- Operators can opt into stricter admission by adding more targets, such as a
  Cloudflare trace URL plus a real overseas business URL.

### R3: Source-level validation survival metrics

Extend fetcher run reports so API/MCP operators can see whether a source is
actually producing usable proxies.

Each fetcher report should include at least:

- number of parsed proxies from that source
- number of unique candidates after batch dedup/source attribution
- number that passed admission validation
- number stored
- validation survival rate

### R4: Preserve source attribution through dedup and validation

Scheduler validation must retain enough source attribution to update per-source
quality reports after the merged dedup/validation/store pipeline.

When the same proxy appears in multiple sources, this MVP may attribute the
surviving proxy to the dedup winner. Multi-source credit assignment is a
follow-up.

### R5: Do not mutate live dev manually

Implementation and validation must not SSH into dev. Deployment validation uses
GitHub Actions, public HTTP API, and MCP HTTP update tools only.

### R6: Avoid paid/provider-specific integration in this slice

Do not add paid proxy providers or provider credentials in this MVP. The task
creates the measurement and stricter admission foundation first.

## Acceptance Criteria

- [ ] Config supports multiple admission validation targets while preserving
      existing single-target behavior.
- [ ] Scheduler can validate candidates against all configured targets.
- [ ] Fetcher reports expose parsed/unique/validated/stored counts and a
      validation survival rate.
- [ ] REST `/api/fetchers` and MCP `fetcher_status` expose the new fields via
      shared core types without adapter-side recomputation.
- [ ] Unit tests cover effective validation target fallback, multi-target
      admission success/failure, and fetcher quality field serialization.
- [ ] Existing route/gateway behavior remains unchanged.
- [ ] No changes are made to `.codex/config.toml`.
- [ ] Dev deploy is verified without SSH.

## Out Of Scope

- Paid proxy provider integrations.
- New public proxy source fetchers, unless the quality foundation is already in
  place and a source can be added with trivial risk.
- Persistent long-term source history in Redis.
- Dynamic refresh scheduling by source quality.
- Gateway route ordering changes.

## Open Question

Should the first implementation use strict all-target admission when multiple
validation targets are configured?

Recommended answer: yes. It makes "high quality" concrete and avoids admitting
proxies that only pass a generic health endpoint but fail real business
targets. The trade-off is fewer stored proxies; if the pool becomes too small,
a later slice can add a configurable threshold such as "2 of 3 targets".
