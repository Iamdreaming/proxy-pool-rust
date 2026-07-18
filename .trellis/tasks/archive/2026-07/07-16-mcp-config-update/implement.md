# Implement: Add MCP tool for runtime config modification

## Checklist

### Step 1: Add `put_json` to `RestClient` (proxy-mcp)
- [ ] Add `pub async fn put_json(&self, path: &str, body: Option<&Value>) -> Result<Value, RestError>` to `crates/proxy-mcp/src/rest_client.rs`
- [ ] Mirror `post_json` implementation
- [ ] Add unit test for PUT method
- **Validate**: `cargo test -p proxy-mcp`

### Step 2: Add `merge_settings_partial` to `proxy-core`
- [ ] Add `pub fn merge_settings_partial(current: &Settings, partial: Value) -> Result<Settings, SettingsEditError>` to `crates/proxy-core/src/config.rs`
- [ ] Logic: deserialize `partial` into `Value`, then for each top-level key present in `partial`, replace that section in `current` via serde round-trip
- [ ] Add unit tests: partial override of subscription section, partial with unknown keys (ignored), empty partial (returns current unchanged)
- **Validate**: `cargo test -p proxy-core`

### Step 3: Add `changed_sections` computation to `proxy-core`
- [ ] Add `pub fn settings_changed_sections(old: &Settings, new: &Settings) -> Vec<String>` to `crates/proxy-core/src/config.rs`
- [ ] Compare each top-level section via `serde_json::to_value` equality check
- [ ] Return list of section names that differ (e.g., `["subscription"]`)
- [ ] Add unit test
- **Validate**: `cargo test -p proxy-core`

### Step 4: Add `reload_config` to `SubscriptionOpsHandle` (proxy-sub)
- [ ] Add `pub async fn reload_config(&self, config: &SubscriptionConfig)` to `SubscriptionOpsHandle` in `crates/proxy-sub/src/ops.rs`
- [ ] Implementation: call `entries_from_config`, acquire write lock on `inner`, swap `entries` and clear `reports`
- [ ] Add unit test (construct handle, call reload_config, verify entry count changes)
- **Validate**: `cargo test -p proxy-sub`

### Step 5: Add `POST /api/subscription/reload` endpoint (proxy-api)
- [ ] Add `SubscriptionReloadResponse` struct: `{ status: String, source_count: usize }`
- [ ] Add `async fn reload_subscription` handler that reads fresh config, calls `subscription_ops.reload_config(&config.subscription)`
- [ ] Register route `.route("/api/subscription/reload", post(reload_subscription))`
- [ ] **Validate**: `cargo test -p proxy-api`

### Step 6: Enhance `PUT /api/settings` to support partial merge + dynamic restart_required (proxy-api)
- [ ] Modify `update_settings` to detect if request body is a partial (doesn't have all top-level keys) vs full settings
- [ ] If partial: call `merge_settings_partial` then `write_settings_for_edit`
- [ ] If full: use existing `write_settings_for_edit` path
- [ ] Compute `changed_sections` via `settings_changed_sections`
- [ ] Set `restart_required` dynamically: `true` if any non-subscription section changed
- [ ] Add `changed_sections` field to `SettingsResponse`
- [ ] **Validate**: `cargo test -p proxy-api`

### Step 7: Add MCP tools (proxy-mcp)
- [ ] Define `UpdateConfigParam` struct: `{ settings: Value }`
- [ ] Add `get_config` tool — delegates to `GET /api/settings`
- [ ] Add `update_config` tool — delegates to `PUT /api/settings`, auto-calls `POST /api/subscription/reload` if subscription changed and restart_required=false
- [ ] Add `reload_subscription_sources` tool — delegates to `POST /api/subscription/reload`
- [ ] **Validate**: `cargo test -p proxy-mcp`

### Step 8: Full validation
- [ ] `cargo test` — all crates, zero failures
- [ ] `cargo clippy -- -D warnings` — zero warnings
- [ ] `cargo fmt` — no formatting changes
- [ ] Manual test via MCP: `get_config` → `update_config` with subscription.search.enabled=true → `reload_subscription_sources` → verify source appears in `subscription_sources`

## Risky Files / Rollback Points

- `crates/proxy-core/src/config.rs` — merge logic must handle serde defaults correctly; test with empty sections
- `crates/proxy-sub/src/ops.rs` — write lock held during entry swap; ensure lock duration is minimal
- `crates/proxy-api/src/routes.rs` — partial vs full detection must not break existing full-settings PUT callers
