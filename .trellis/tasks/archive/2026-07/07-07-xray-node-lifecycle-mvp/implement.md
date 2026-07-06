# Implement: xray node lifecycle MVP

## Steps

1. Read the relevant Trellis specs before coding:
   - `.trellis/spec/guides/index.md`
   - `.trellis/spec/proxy-core/backend/index.md`
   - `.trellis/spec/proxy-xray/backend/index.md`
   - `.trellis/spec/proxy-api/backend/index.md`
   - `.trellis/spec/proxy-mcp/backend/index.md`
   - `.trellis/spec/proxy-server/backend/index.md`

2. Add shared lifecycle models and registry in `proxy-core`.
   - Keep serde output stable and snake_case where existing JSON uses it.
   - Add unit tests for state transitions and snapshot counts.

3. Wire `OutboundSync` to the registry.
   - Mark `activating`, `active`, `failed`, and `removed` at the real decision
     points.
   - Do not treat partial xray gRPC failure as active.
   - Release allocated ports on failure paths.

4. Wire `proxy-server`.
   - Create the registry.
   - Pass it to `OutboundSync`, `proxy-api::AppState`, and `ProxyPoolMcpConfig`.

5. Update API/MCP/status surfaces.
   - Expand `/api/xray/status`.
   - Expand `service_status` xray summary.
   - Add or update MCP xray status output.

6. Update docs after implementation.
   - README endpoint/tool descriptions if JSON shape changes materially.
   - `docs/ROADMAP.md` status after the task is complete.

7. Verify.
   - `cargo fmt --all --check`
   - `cargo test --workspace --all-targets`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - Push and watch GitHub Actions.
   - No-SSH HTTP/MCP smoke check.
