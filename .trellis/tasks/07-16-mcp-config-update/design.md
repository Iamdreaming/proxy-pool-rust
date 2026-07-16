# Design: Add MCP tool for runtime config modification

## Architecture

### Approach: MCP → REST API → File + Hot Reload

The MCP tools delegate to the existing REST API for config read/write (adding a `PUT` method to `RestClient`). A new REST endpoint `POST /api/subscription/reload` triggers the subscription hot-reload. The MCP `update_config` tool calls both the PUT and the reload as needed.

```
MCP tool (proxy-mcp)
  ├── get_config          → GET  /api/settings
  ├── update_config       → PUT  /api/settings  (+ POST /api/subscription/reload if subscription changed)
  └── reload_subscription → POST /api/subscription/reload
```

### Why delegate to REST API instead of direct file access?

1. The MCP server is a separate process; it has no access to `AppState` or `SubscriptionOpsHandle`.
2. The REST API already has `read_settings_for_edit`/`write_settings_for_edit` with validation, redaction, and atomic file writes.
3. The REST API runs in the same process as `SubscriptionOpsHandle`, so it can trigger hot-reload directly.
4. Consistency: all existing MCP tools that need live data already delegate to REST.

## Component Changes

### 1. `proxy-core/src/config.rs` — Add partial-merge helper

Add `merge_settings_partial(current: &Settings, partial: Value) -> Result<Settings>` that:
- Takes the current settings and a JSON `Value` containing only the fields to change.
- Deserializes the partial into a `Settings` with `#[serde(default)]` filling missing fields.
- Merges: for each top-level section present in the partial, replace the current section entirely.
- Returns the merged `Settings` for validation and persistence.

This is needed because the REST API currently requires a full `Settings` object. The MCP tool should accept a partial JSON and merge it with current settings server-side.

### 2. `proxy-api/src/routes.rs` — Enhance PUT /api/settings + Add reload endpoint

**Enhance `update_settings`**:
- Accept both full `Settings` (existing) and partial JSON merge (new).
- Add `changed_sections` to `SettingsResponse` — compute by comparing top-level section fields between old and new settings.
- Change `restart_required` from hardcoded `true` to dynamic: `true` if any non-subscription section changed, `false` if only subscription changed.

**Add `POST /api/subscription/reload`**:
- Reads fresh config from YAML via `read_settings_for_edit`.
- Calls `SubscriptionOpsHandle::reload_config(&new_config)`.
- Returns `{ "status": "ok", "source_count": N }`.

### 3. `proxy-sub/src/ops.rs` — Add `reload_config` to `SubscriptionOpsHandle`

Add method:
```rust
pub async fn reload_config(&self, config: &SubscriptionConfig) {
    let new_entries = entries_from_config(config, Some(self.store.clone()));
    let mut inner = self.state.inner.write().await;
    inner.entries = new_entries;
    inner.reports.clear();
}
```

This rebuilds the entry list from fresh config while holding the write lock briefly. The background refresh loop will pick up new entries on its next cycle, or the operator can call `refresh_all` immediately.

### 4. `proxy-mcp/src/rest_client.rs` — Add `put_json`

Add `PUT` support mirroring `post_json`:
```rust
pub async fn put_json(&self, path: &str, body: Option<&Value>) -> Result<Value, RestError>
```

### 5. `proxy-mcp/src/lib.rs` — Add 3 MCP tools

**`get_config`** (no params):
- Calls `self.rest.get_json("/api/settings", &[])`.
- Returns the response JSON.

**`update_config`** (param: `UpdateConfigParam { settings: Value }`):
- Calls `self.rest.put_json("/api/settings", Some(&params.settings))`.
- If the response indicates subscription changed and `restart_required` is false, automatically calls `POST /api/subscription/reload`.
- Returns the settings response with `restart_required` and `changed_sections`.

**`reload_subscription_sources`** (no params):
- Calls `self.rest.post_json("/api/subscription/reload", None)`.
- Returns the reload result.

## Data Flow

### update_config flow

```
1. MCP tool receives partial JSON: { "subscription": { "search": { "enabled": true, "mcp_url": "..." } } }
2. MCP calls PUT /api/settings with the partial JSON
3. REST handler:
   a. Reads current settings from YAML
   b. Merges partial into current (section-level replace)
   c. Validates merged settings
   d. Writes to YAML atomically
   e. Computes changed_sections, sets restart_required
   f. Returns SettingsResponse
4. MCP checks response: if subscription changed and restart_required=false
   → MCP calls POST /api/subscription/reload
5. REST reload handler:
   a. Reads fresh config from YAML
   b. Calls SubscriptionOpsHandle::reload_config(&new_subscription_config)
   c. Returns { status: "ok", source_count: N }
6. MCP returns combined result to caller
```

## Contracts

### MCP tool: `update_config`

**Input** (`UpdateConfigParam`):
```json
{
  "settings": {
    // Partial Settings JSON — only sections to change
    "subscription": {
      "search": { "enabled": true, "mcp_url": "http://host:33000/mcp/search" }
    }
  }
}
```

**Output**:
```json
{
  "status": "ok",
  "restart_required": false,
  "changed_sections": ["subscription"],
  "redacted_fields": ["subscription.github.token"],
  "settings": { /* full redacted settings */ }
}
```

### MCP tool: `reload_subscription_sources`

**Input**: none

**Output**:
```json
{
  "status": "ok",
  "source_count": 5
}
```

### REST: `POST /api/subscription/reload`

**Input**: none

**Output**:
```json
{
  "status": "ok",
  "source_count": 5
}
```

## Compatibility & Migration

- Existing `PUT /api/settings` with full `Settings` body continues to work unchanged.
- The partial-merge path is additive: if the body has all top-level sections, it behaves identically to the current full-replace path.
- `SettingsResponse` gains `changed_sections: Vec<String>` (default empty for backward compat) and `restart_required` becomes dynamic instead of hardcoded.
- No breaking changes to existing MCP tools or REST endpoints.

## Trade-offs

1. **Section-level merge vs field-level merge**: We replace entire top-level sections (e.g., all of `subscription`) rather than deep-merging individual fields. This is simpler and avoids partial-update ambiguities (e.g., what happens to `subscription.urls` when only `subscription.search` is sent?). The downside is that the caller must include the full section they want to modify. This is acceptable because the MCP tool can first `get_config`, modify the desired section, then `update_config`.

2. **Auto-reload vs manual reload**: `update_config` automatically calls `reload_subscription_sources` when subscription changes. This is more convenient but means the MCP tool makes two REST calls. The standalone `reload_subscription_sources` tool exists for cases where the operator wants to reload without changing config (e.g., after manually editing the YAML).

3. **Hot-reload scope limited to subscription**: Other subsystems (scheduler, WARP, gateway) would require more complex re-initialization. Subscription is the simplest case because `SubscriptionOpsHandle` already holds `Arc<RwLock<SubscriptionOpsInner>>` — we just need to swap the entries vector.

## Rollback

- `write_settings_for_edit` already creates a `.bak` file before writing.
- If `reload_config` fails, the old entries remain in memory (the write lock is only released after successful swap).
- If the merged settings fail validation, the YAML is not written and the error is returned.
