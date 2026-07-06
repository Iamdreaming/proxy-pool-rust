# Implement: xray config dry-run and remove

## Steps

1. Read relevant Trellis specs before coding:
   - `.trellis/spec/guides/index.md`
   - `.trellis/spec/proxy-xray/backend/index.md`
   - `.trellis/spec/proxy-api/backend/index.md`
   - `.trellis/spec/proxy-mcp/backend/index.md`
   - `.trellis/spec/proxy-server/backend/index.md`

2. Add xray operator models/handle in `proxy-xray`.
   - Reuse `ConfigGenerator::generate`.
   - Return sanitized metadata only.
   - Add unit tests.

3. Extend `OutboundSync`.
   - Add a method to remove a tracked active node by tag.
   - Release port and remove `ProxyStore` entry.
   - Update lifecycle status.
   - Keep stale-node cleanup behavior compatible.

4. Wire `proxy-server`.
   - Construct `XrayOpsHandle` when xray is enabled.
   - Pass it to API and MCP.
   - Preserve disabled xray behavior.

5. Add API/MCP surfaces.
   - REST dry-run and remove endpoints.
   - MCP dry-run and remove tools.
   - Use shared result models.

6. Update docs/spec/tests.
   - README endpoint/tool list.
   - Integration tests for API/MCP contracts.
   - Trellis spec updates for xray ops pattern.

7. Verify.
   - `cargo fmt --all --check`
   - `cargo test -p proxy-xray`
   - `cargo check -p proxy-api -p proxy-mcp -p proxy-server`
   - `cargo test --workspace --all-targets`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - Push and watch GitHub Actions.
   - No-SSH HTTP/MCP smoke check.
