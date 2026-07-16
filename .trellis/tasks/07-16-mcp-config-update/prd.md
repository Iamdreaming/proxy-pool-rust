# PRD: Add MCP tool for runtime config modification

## Goal

Allow operators to modify proxy-pool configuration through the MCP interface, with changes taking effect immediately for subscription sources or after a controlled restart for other sections. The immediate use case is enabling/configuring the search discoverer without manually editing YAML and restarting.

## Background

- Config is loaded once at startup from YAML (`Settings` struct). No hot-reload mechanism exists.
- The REST API already has `GET/PUT /api/settings` that reads/writes the YAML file atomically (`read_settings_for_edit`/`write_settings_for_edit` in `proxy-core/src/config.rs`), but changes do NOT propagate to running subsystems — they use startup clones.
- `SubscriptionOpsHandle` (`proxy-sub/src/ops.rs`) is constructed once from `SubscriptionConfig`; `entries_from_config` is only called at construction. No runtime update method exists.
- The MCP server (`proxy-mcp`) already delegates many tools to the REST API via `RestClient`.

## Requirements

### REQ-1: Read current config via MCP

Expose the full (redacted) settings through an MCP tool, so operators can inspect the live configuration.

### REQ-2: Update config via MCP (generic merge)

A single `update_config` tool accepts a partial JSON object that is merged into the current settings. Only specified fields are overwritten; unspecified fields retain their current values. Changes are validated and persisted to the YAML file atomically.

### REQ-3: Runtime reload for subscription sources

After updating subscription-related config, operators can call `reload_subscription_sources` to rebuild the `SubscriptionOpsHandle` entry list from the updated config. The subscription refresh cycle picks up new sources immediately.

### REQ-4: Restart awareness

The `update_config` response indicates whether the changed sections require a restart. Only `subscription` changes are hot-reloadable in v1; all other sections are flagged as `restart_required: true`.

## Acceptance Criteria

- AC-1: `get_config` MCP tool returns current redacted settings JSON.
- AC-2: `update_config` MCP tool accepts a partial JSON object, validates, persists to YAML, and returns the result with `restart_required` indicator and `changed_sections` list.
- AC-3: `reload_subscription_sources` MCP tool triggers `SubscriptionOpsHandle` to rebuild entries from the updated config; new sources appear in `subscription_sources` status immediately.
- AC-4: Non-subscription changes are flagged with `restart_required: true` in the `update_config` response.
- AC-5: `cargo test` and `cargo clippy -- -D warnings` pass with zero failures/warnings.
- AC-6: Config validation (port ranges, non-empty URLs, score ranges) is applied on write via existing `validate_settings`.

## Out of Scope

- Hot-reload for non-subscription config sections (pool, gateway, WARP, etc.) — future iteration.
- Config change notification/webhook system.
- Config version history or rollback beyond the `.bak` file.
- UI or dashboard for config editing.
