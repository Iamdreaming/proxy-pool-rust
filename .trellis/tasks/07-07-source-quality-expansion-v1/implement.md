# Implementation Plan: source-quality-expansion-v1

## Checklist

1. [x] Load `trellis-before-dev` and read applicable specs:
   - `proxy-core/backend`
   - `proxy-api/backend`
   - `proxy-mcp/backend`
   - shared guides
2. [x] Update `PoolSettings` with backward-compatible
   `validate_target_urls`.
3. [x] Add an effective validation target helper and tests.
4. [x] Add scheduler admission validation for one or many targets.
5. [x] Extend `FetcherRunReport` with source quality fields.
6. [x] Update scheduler attribution:
   - parsed/fetched remains per fetcher output
   - `unique` counted after batch dedup
   - `validated` counted after admission validation
   - `stored` counted after Redis store success
7. [x] Ensure REST and MCP fetcher status serialize new fields without local
   recomputation.
8. [x] Update config example and relevant Trellis specs.
9. [x] Run local verification:
   - `cargo fmt --all --check`
   - `cargo test -p proxy-core fetcher`
   - `cargo test -p proxy-core scheduler`
   - `cargo test -p proxy-core validator`
   - `cargo test -p proxy-api fetcher`
   - `cargo test -p proxy-mcp fetcher`
   - `cargo clippy -p proxy-core -p proxy-api -p proxy-mcp -- -D warnings`
   - `cargo check --workspace`
10. [ ] Commit and push.
11. [ ] Wait for GitHub Actions Docker build.
12. [ ] Update dev through MCP HTTP `update_service`.
13. [ ] Verify dev with read-only smoke and fetcher status shape.

## Risk Points

- Multi-target validation can multiply outbound connection attempts. Keep
  proxy-level concurrency bounded and avoid unbounded per-target fan-out in the
  MVP.
- Strict admission can shrink the pool. Existing behavior is preserved unless
  operators configure multiple targets.
- Fetcher report fields are shared API/MCP contracts; adapters must not invent
  or recompute them.

## Rollback Points

- Config rollback: remove `pool.validate_target_urls` to return to current
  single-target behavior.
- Code rollback: revert the scheduler multi-target admission helper while
  keeping report fields optional/defaulted if consumers have already adopted
  them.
