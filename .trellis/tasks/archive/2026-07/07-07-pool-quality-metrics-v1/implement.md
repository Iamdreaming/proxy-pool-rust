# Implementation Plan: pool-quality-metrics-v1

1. Add quality status structs and aggregation helpers in `proxy-core::status`.
2. Wire `collect_service_status()` to include quality and preserve Redis error
   reporting when quality collection fails.
3. Extend `render_prometheus_metrics()` with low-cardinality quality metrics.
4. Add core tests for quality aggregation and metric rendering.
5. Update REST and MCP integration shape tests for `quality`.
6. Update README/roadmap/spec docs only where the new operator contract is
   user-visible or reusable.
7. Run validation:
   - `cargo fmt --all --check`
   - `cargo test -p proxy-core`
   - `cargo test -p proxy-api`
   - `cargo test -p proxy-mcp`
   - `cargo test --workspace --all-targets`
   - `cargo clippy --workspace --all-targets -- -D warnings`
