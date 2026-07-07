# Free Source Expansion v2

## Goal

Increase the proxy candidate supply by adding several legal, public, free proxy
sources that can be fetched without accounts, registration automation, or
service abuse.

## Scope

- Add new built-in fetchers for public raw/JSON proxy lists:
  - Proxifly free-proxy-list
  - Databay Labs free-proxy-list
  - IPLocate free-proxy-list
  - VPSLab Free Proxy List
  - Monosans proxy-list
- Keep every source behind a normal `FetcherToggle` config entry.
- Reuse the existing scheduler, source circuit breaker, validation, scoring,
  and fetcher status reporting path.
- Support source formats observed during research:
  - `host:port`
  - `scheme://host:port`
  - comment/blank-line text lists
  - JSON arrays containing `protocol`, `host`, and `port`
- Keep source ids stable and machine-readable for `fetcher_status` and
  `refresh_fetcher`.

## Non-Goals

- No VPN account registration automation.
- No CAPTCHA, email, phone, invite, or quota bypass.
- No paid-provider integration.
- No target-specific exit scoring in this task.
- No direct SSH or host Docker validation.

## Acceptance Criteria

- Config can enable/disable each new source independently.
- `build_fetchers` includes the new sources when enabled and excludes them when
  disabled.
- Parser unit tests cover text lists, URL-style proxy entries, comments,
  invalid rows, and Monosans JSON entries.
- `config/settings.example.yaml` documents the new toggles.
- Local checks pass:
  - `cargo fmt --all --check`
  - `cargo test -p proxy-core fetcher`
  - `cargo test -p proxy-core config`
  - `cargo clippy -p proxy-core -- -D warnings`
  - `cargo check --workspace`

## Risks

- Free public lists can be noisy and duplicate-heavy. This task only increases
  candidate supply; existing validation and source survival metrics decide what
  is retained.
- GitHub/raw endpoints can fail or be rate-limited. The existing source circuit
  breaker should isolate repeated source failures.
