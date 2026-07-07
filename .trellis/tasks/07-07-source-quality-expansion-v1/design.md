# Design: source-quality-expansion-v1

## Architecture

This task keeps the existing fetcher and gateway architecture intact.

```text
fetchers -> Scheduler::run_selected
  -> batch dedup
  -> admission validation against effective target list
  -> source-level quality attribution
  -> Redis store
  -> API/MCP fetcher status serialization
```

`proxy-core` remains the owner of fetcher reports, validation rules, and source
quality calculations. API and MCP adapters should only serialize shared core
types.

## Config Contract

Add a new optional setting under `pool`:

```yaml
pool:
  validate_target_url: "https://www.cloudflare.com/cdn-cgi/trace"
  validate_target_urls:
    - "https://www.cloudflare.com/cdn-cgi/trace"
    - "https://httpbin.org/ip"
```

Compatibility rule:

- if `validate_target_urls` is empty or omitted, use `[validate_target_url]`
- if `validate_target_urls` is non-empty, use that list

This preserves current deployments by default and lets dev/prod opt into
stricter admission through config only.

## Admission Validation

The scheduler currently owns batch validation through `Validator::validate_many`.
For multi-target admission, add a core helper that validates one proxy against
all effective targets and returns a validated `Proxy` only if every target
succeeds.

The helper should reuse `Validator` and keep the same timeout/concurrency
semantics:

- concurrency still caps proxy-level validation tasks
- target checks for one proxy may run sequentially in the MVP to avoid
  multiplying outbound connection spikes
- the accepted proxy should carry useful latency/anonymity metadata from the
  validation checks

The default one-target path should remain behaviorally equivalent to the
current implementation.

## Source Quality Attribution

Current fetchers already set `Proxy.source`. After batch dedup, the scheduler
can count candidates by `source` before validation and count accepted/stored
proxies by the same source after validation/store.

Extend `FetcherRunReport` with source quality fields:

- `unique`: number of deduplicated candidates attributed to this fetcher
- `validated`: number of attributed candidates admitted by validation
- `stored`: number of attributed proxies stored successfully
- `validation_survival_rate`: `validated / unique`, omitted or `null` when
  `unique == 0`

The report update should happen in `proxy-core::scheduler`, after the pipeline
knows dedup, validation, and store outcomes. API/MCP should receive the same
new fields automatically through `FetcherRunReport`.

## Trade-Offs

Strict all-target admission improves confidence but reduces pool size. That is
acceptable for this MVP because the current failure mode is not lack of volume;
it is many low-quality proxies that pass weak checks but fail gateway traffic.

Per-source credit for duplicate proxies is simplified: the dedup winner gets
credit. Multi-source credit assignment is more complex and should wait until
the basic source quality loop proves useful.

## Rollout And Rollback

No migration is required. Existing configs do not set `validate_target_urls`, so
single-target behavior remains the default.

Rollback is straightforward:

- remove extra `validate_target_urls` from deployment config to return to
  single-target admission
- if needed, revert the source quality fields while keeping old fetcher status
  consumers compatible through serde defaults/optional fields

