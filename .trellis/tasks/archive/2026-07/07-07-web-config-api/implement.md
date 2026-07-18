# Implementation Plan

## Checklist

- [ ] Add `config_path` to `proxy_api::AppState` and pass the startup config path from `proxy-server`.
- [ ] Add backend settings response/request structs and `GET /api/settings`, `PUT /api/settings` routes.
- [ ] Implement helpers for loading settings from path, redacting sensitive fields, merging redacted placeholders, validating settings, and safe YAML write.
- [ ] Add backend tests for redaction/merge behavior, validation failures, and invalid input not overwriting existing config.
- [ ] Add frontend settings types and API wrappers.
- [ ] Replace `web/src/views/Settings.vue` placeholder with a real loading/error/editor/save flow.
- [ ] Run focused backend tests.
- [ ] Run frontend build or type-check/build equivalent.

## Validation Commands

- `cargo test -p proxy-api`
- `cargo test -p proxy-core config`
- `npm run build --prefix web`

## Rollback Points

- Revert `AppState` shape and server construction if config path propagation causes compile failures.
- Revert only `/api/settings` routes if backend helper tests pass but API integration causes router issues.
- Revert `Settings.vue` separately if frontend build issues are isolated to UI wiring.

## Review Gate

Implementation may start after user approval and `python ./.trellis/scripts/task.py start 07-07-web-config-api`.
