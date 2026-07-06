# Implementation Plan: proxy-quality-history-lite

1. [x] Read the relevant Trellis specs for `proxy-core`, `proxy-api`, and
   `proxy-mcp`.
2. [x] Inspect current `Proxy`, `ProxyStore`, score explanation, API scores route,
   and MCP score tools.
3. [x] Add compact quality history structs and helper methods in `proxy-core`.
4. [x] Update store success/failure paths to append bounded samples.
5. [x] Add trend summary to score explanations.
6. [x] Update API/MCP serialization tests for the new JSON contract.
7. [x] Update docs/specs if the score explanation contract changes.
8. [x] Run focused tests, then workspace test and clippy.
9. [x] Update Roadmap and archive the Trellis task when verified.

## Expected Files

- `crates/proxy-core/src/models.rs`
- `crates/proxy-core/src/store.rs`
- `crates/proxy-api/src/routes.rs`
- `crates/proxy-mcp/src/lib.rs`
- `docs/score-retention.md`
- `.trellis/spec/proxy-core/backend/quality-guidelines.md`
- `docs/ROADMAP.md`
