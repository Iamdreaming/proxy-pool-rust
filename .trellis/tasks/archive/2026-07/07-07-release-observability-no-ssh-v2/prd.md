# PRD: release-observability-no-ssh-v2

## Background

The project already has a CI -> GHCR -> Watchtower/MCP update path and public
status endpoints. Operators still need a clearer no-SSH way to answer:

- Which image/tag/digest is this process configured to run?
- What container/image was observed during the last update attempt?
- Has `update_service` never been called, already been current, succeeded, or
  failed?
- Which public checks should be run after a push?

## Goal

Expose enough release and update state through public REST/MCP surfaces to verify
dev release progress without direct SSH access.

## Non-Goals

- No direct SSH or host Docker commands from this task.
- No destructive fault injection.
- No dev compose edits or token changes.
- No automatic rollback implementation.
- No Dashboard UI work; `dashboard-ops-polish-v2` remains paused.

## Requirements

### F1: Release metadata in public status

- `/api/status` and MCP `service_status` should include a structured release
  metadata object.
- Metadata should include app version, git hash, configured image, update target
  image, update container name, update enabled flag, and build-time hints when
  available.
- Fields must be optional where runtime/build metadata is not available.

### F2: Last update result query

- `update_service` should persist the most recent update attempt result in
  process memory.
- A new public read-only MCP tool should return the latest update status.
- The response must clearly distinguish `never_triggered`, `already_current`,
  `updated`, `failed`, and `disabled`.
- The response should include observed before/after container image IDs/digests
  when available and an error string on failure.

### F3: No-SSH release validation docs

- `docs/dev-validation.md` should document the post-push validation order.
- README should mention the release metadata and last update status surfaces.
- The docs should prefer GitHub Actions, `/api/status`, and MCP tools, not SSH.

## Acceptance Criteria

- [x] `/api/status` includes release metadata without requiring Docker socket
  access.
- [x] MCP `service_status` returns the same release metadata.
- [x] MCP exposes a read-only latest update status tool.
- [x] `update_service` records latest result for success, already-current,
  disabled, and error paths that can be observed without SSH.
- [x] README and `docs/dev-validation.md` describe the no-SSH post-push flow.
- [x] Relevant unit tests pass.
- [x] Workspace-wide formatting and targeted package tests pass.

## Verification

- `cargo fmt --all --check`
- `cargo test -p proxy-core`
- `cargo test -p proxy-mcp`
- `cargo test -p proxy-api`
- `cargo check -p proxy-server`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
