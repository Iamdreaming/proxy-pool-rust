# Settings Edit API Contract

## Scenario: Web Settings Read And Write

### 1. Scope / Trigger

- Trigger: Web operators need to view and edit the YAML service settings through REST and the dashboard.
- This contract spans `proxy-core`, `proxy-api`, `proxy-server`, and `web`.
- `proxy-core::config` owns parsing, redaction, redacted-placeholder merge, validation, and YAML persistence. API and UI layers must not duplicate that logic.

### 2. Signatures

- Core strict read: `read_settings_for_edit(path) -> Result<Settings, SettingsEditError>`.
- Core display redaction: `redact_settings(settings) -> (Settings, Vec<String>)`.
- Core placeholder merge: `merge_redacted_settings(submitted, current) -> Settings`.
- Core validation: `validate_settings(settings) -> Result<(), SettingsEditError>`.
- Core write: `write_settings_for_edit(path, submitted) -> Result<Settings, SettingsEditError>`.
- API state: `AppState.config_path: PathBuf`, set from the same startup config path used by `load_settings()`.
- REST read: `GET /api/settings`.
- REST write: `PUT /api/settings` with body `{ "settings": <Settings object> }`.
- Frontend helpers: `fetchSettings()` and `updateSettings(settings)`.

### 3. Contracts

`GET /api/settings` returns:

| Field | Type | Meaning |
|-------|------|---------|
| `status` | string | `"ok"` on success |
| `path` | string | YAML config path used at process startup |
| `restart_required` | boolean | Always `true` until full runtime hot reload exists |
| `redacted_fields` | string array | Dot paths replaced by `__PROXY_POOL_REDACTED__` |
| `settings` | object | `Settings` serialized as JSON, with supported sensitive fields redacted |

`PUT /api/settings` accepts the full `Settings` object and returns the same response shape after a successful write.

Sensitive placeholder:

```text
__PROXY_POOL_REDACTED__
```

Supported sensitive fields:

| Field | Rule |
|-------|------|
| `redis.url` | Redacted on read when non-empty; placeholder on write preserves the current file value |
| `subscription.github.token` | Redacted on read when present and non-empty; placeholder on write preserves the current file value; `null` clears the token |

YAML writes are normalized through `serde_yaml`; preserving comments and manual formatting is not part of this contract.

### 4. Validation & Error Matrix

| Condition | Contract |
|-----------|----------|
| Config file missing on read | Return `Settings::default()` as redacted JSON |
| Existing config file cannot be read | API returns HTTP 500 with `SimpleResponse` |
| Existing config file is invalid YAML | API returns HTTP 500; do not replace the file with defaults |
| Request body is malformed JSON | Axum returns HTTP 400 before the handler runs |
| Submitted settings fail `validate_settings` | API returns HTTP 400 and the original YAML is not touched |
| Submitted sensitive field equals the redacted placeholder | Preserve the current YAML value |
| Submitted sensitive field has a new concrete value | Write the new concrete value |
| Submitted GitHub token is `null` | Clear the token |
| YAML serialization or file replace fails | API returns HTTP 500; do not log request bodies or secrets |
| Write succeeds | Return redacted response with `restart_required=true` |

### 5. Good/Base/Bad Cases

- Good: API handlers call `read_settings_for_edit`, `write_settings_for_edit`, and `redact_settings`, then only map results into HTTP status codes.
- Good: UI saves the full JSON settings object it received, leaving redacted placeholders unchanged when the operator did not intend to rotate secrets.
- Base: missing `config/settings.yaml` behaves like startup config loading and exposes default settings for editing.
- Bad: API or UI implements its own secret merge rules. That can write the redacted placeholder into real YAML or drift from core validation.
- Bad: returning startup defaults when an existing YAML file is malformed. That makes accidental overwrite too easy.
- Bad: claiming settings apply immediately before runtime hot reload exists.

### 6. Tests Required

- `proxy-core` unit tests for redaction paths and field list.
- `proxy-core` unit tests for placeholder merge preserving current sensitive values.
- `proxy-core` unit tests proving invalid submitted settings do not overwrite existing YAML.
- `proxy-api` serialization test for `SettingsResponse`.
- `proxy-api` request deserialization test proving partial JSON settings fill serde defaults.
- Frontend type-check/build proving `fetchSettings`, `updateSettings`, and the Settings view agree on the response shape.

### 7. Wrong vs Correct

#### Wrong

```rust
// API layer owns redaction and can drift from config persistence.
let mut settings = load_settings(&state.config_path);
settings.redis.url = "__redacted__".into();
Json(settings)
```

#### Correct

```rust
let settings = read_settings_for_edit(&state.config_path)?;
let (settings, redacted_fields) = redact_settings(&settings);
Json(SettingsResponse {
    status: "ok".into(),
    path: state.config_path.display().to_string(),
    restart_required: true,
    redacted_fields,
    settings,
})
```
