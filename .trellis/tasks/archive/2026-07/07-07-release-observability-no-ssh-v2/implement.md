# Implementation Plan: release-observability-no-ssh-v2

1. Inspect existing status, update, API, MCP, and docs paths.
2. Add `ReleaseMetadata` to the shared status response.
3. Populate release metadata from existing runtime/build configuration.
4. Add process-local latest update status state and a read-only MCP tool.
5. Record latest result in all `update_service` return paths.
6. Update README, `docs/dev-validation.md`, Roadmap, and task acceptance.
7. Run focused tests, formatting, and workspace checks appropriate to the
   touched packages.

## Expected Files

- `crates/proxy-core/src/status.rs`
- `crates/proxy-server/src/main.rs`
- `crates/proxy-mcp/src/*`
- `crates/proxy-api/src/*`
- `README.md`
- `docs/dev-validation.md`
- `docs/ROADMAP.md`
- `.trellis/tasks/07-07-release-observability-no-ssh-v2/*`
