# Implementation Plan: web-dashboard-real-ops-mvp

## Steps

- [x] Add frontend API types and typed helper functions for readiness, scored
      proxies, route dry-run, fetcher status, and fetcher refresh.
- [x] Rework Dashboard overview to show real status/readiness and useful
      loading/error states.
- [x] Add score columns/data loading to Proxies page.
- [x] Add route dry-run panel to Routes page.
- [x] Add Fetchers page plus router/sidebar entries.
- [x] Update MCP Debug tool catalog and execution behavior.
- [x] Replace simulated Logs behavior with a truthful unavailable state.
- [x] Run build and relevant tests.
- [x] Update Roadmap/task status and prepare commits without including
      `.codex/config.toml`.

## Validation Commands

```powershell
cd web
npm run build
```

If Rust API files are touched:

```powershell
cargo test -p proxy-api --lib
```

Before final commit:

```powershell
git status --short
git diff --check
```

## Risk Points

- Frontend types must match Rust JSON field names exactly.
- Existing API error behavior is not fully uniform; UI must handle non-2xx and
  valid empty payloads.
- Do not add fake log data just to fill the page.
- Do not include unrelated `.codex/config.toml` changes.
- Do not use direct SSH for dev validation.
