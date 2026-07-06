# Implement: subscription source ops MVP

## Steps

1. Read relevant Trellis specs before coding:
   - `.trellis/spec/guides/index.md`
   - `.trellis/spec/proxy-sub/backend/index.md`
   - `.trellis/spec/proxy-api/backend/index.md`
   - `.trellis/spec/proxy-mcp/backend/index.md`
   - `.trellis/spec/proxy-server/backend/index.md`

2. Add structured report models in `proxy-sub`.
   - Keep serde output snake_case where needed.
   - Redact or omit node secrets.
   - Add tests for counts and serialization.

3. Refactor refresh execution.
   - Keep existing `run_refresh_cycle` compatibility.
   - Add an ops-friendly refresh function that can run all sources or one source.
   - Support `Preview` mode that performs no writes.
   - Update shared `SubscriptionOpsState` with latest reports.

4. Wire `proxy-server`.
   - Construct shared subscription ops state.
   - Pass it to the background loop, REST API, and MCP server.
   - Keep Redis failure behavior: log and disable subscription refresh if the
     pending store connection cannot be opened.

5. Add API and MCP surfaces.
   - REST: status list and manual refresh endpoint.
   - MCP: status tool and manual refresh tool.
   - Default manual refresh to dry-run/preview.

6. Update docs/spec/tests.
   - README endpoint/tool list.
   - Integration tests for API/MCP contracts.
   - Trellis spec updates for the new subscription ops pattern.

7. Verify.
   - `cargo fmt --all --check`
   - `cargo test -p proxy-sub`
   - `cargo check -p proxy-api -p proxy-mcp -p proxy-server`
   - `cargo test --workspace --all-targets`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `npm run build` if web types or UI change.
   - Push and watch GitHub Actions.
   - No-SSH HTTP/MCP smoke check.
