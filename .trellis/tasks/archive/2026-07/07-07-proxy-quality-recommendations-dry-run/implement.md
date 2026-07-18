# Implementation Plan: proxy-quality-recommendations-dry-run

Status: paused on 2026-07-07 by user request. Do not execute this plan or make
code changes for this task until the user explicitly resumes it.

1. Read Trellis specs for `proxy-core`, `proxy-api`, and `proxy-mcp`.
2. Inspect current score explanation, cleanup, API scores, and MCP score tools.
3. Add core recommendation types and pure rule helper.
4. Add `ProxyStore::recommend_quality_actions`.
5. Add REST route and response serialization tests.
6. Add MCP tool and parameter/JSON tests.
7. Update docs, Roadmap, and proxy-core spec.
8. Run focused tests, workspace tests, clippy, and integration syntax checks.
9. Archive task, commit, push, and watch GitHub Actions.

## Expected Files

- `crates/proxy-core/src/store.rs`
- `crates/proxy-api/src/routes.rs`
- `crates/proxy-mcp/src/lib.rs`
- `docs/score-retention.md`
- `.trellis/spec/proxy-core/backend/quality-guidelines.md`
- `tests/integration/test_l2_api.py`
- `tests/integration/test_l4_mcp.py`
- `docs/ROADMAP.md`
