# PRD: proxy-quality-recommendations-dry-run

## Background

`proxy-quality-history-lite` made recent proxy trend visible in score
explanations. Operators can now inspect the data, but still need a compact
answer to: "which proxies deserve attention, and why?"

## Goal

Add read-only dry-run quality recommendations that classify stored proxies using
the core score explanation and recent trend data.

## Non-Goals

- Do not delete, refresh, disable, or mutate proxies.
- Do not add an apply mode in this task.
- Do not restore paused Dashboard, contract-smoke, update-failure, WARP, or xray
  tasks.
- Do not require direct SSH, host Docker, or `update_service` for verification.

## Requirements

### F1: Core-owned recommendation contract

- Add a `proxy-core` model for recommendation results.
- Each recommendation must include the proxy, score explanation, suggested
  action, severity, reasons, and dry-run marker.
- Rules must consider current score/retention, recent success rate, recent
  latency, recent failures, and cumulative fail/success counters.

### F2: Read-only API/MCP surfaces

- Add a REST endpoint for dry-run recommendations.
- Add an MCP tool for dry-run recommendations.
- Both surfaces must call the same `proxy-core` store helper and serialize the
  same result type.

### F3: Conservative rule behavior

- The default result should focus on non-keep recommendations.
- Operators can request a limit and protocol.
- No write path, apply flag, Docker, Watchtower, SSH, or scheduler refresh is
  involved.

### F4: Verification

- Add core tests for recommendation rules.
- Add API/MCP serialization and parameter tests.
- Add integration shape assertions for the new REST/MCP read-only surfaces.

## Acceptance Criteria

- [ ] `proxy-core` exposes a dry-run recommendation result model and helper.
- [ ] Recommendations include stable action/severity/reason fields.
- [ ] Rules consider score, retention decision, recent success rate, recent
  latency, recent failures, and cumulative counters.
- [ ] REST and MCP expose read-only recommendation surfaces without adapter-side
  scoring logic.
- [ ] The implementation cannot mutate proxy storage.
- [ ] Docs and Roadmap reflect the new surface and next TODO.
- [ ] Tests and clippy pass locally; verification uses no SSH and no
  `update_service`.
