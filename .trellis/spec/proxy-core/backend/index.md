# proxy-core Spec Index

Guidelines for the `proxy-core` crate — the core library containing data models,
configuration, fetchers, validator, store, scheduler, GeoIP, router, circuit
breaker, WARP integration, dedup, EWMA, and pacing.

| Guide | Description |
|-------|-------------|
| [Directory Structure](./directory-structure.md) | Module layout, sub-module organisation, and role of each file |
| [Error Handling](./error-handling.md) | `anyhow` vs `thiserror`, error propagation, forbidden patterns |
| [Quality Guidelines](./quality-guidelines.md) | Clippy rules, forbidden patterns, Redis storage conventions, testing |
| [Logging Guidelines](./logging-guidelines.md) | `tracing` usage, log-level conventions, what to log and what not to |
