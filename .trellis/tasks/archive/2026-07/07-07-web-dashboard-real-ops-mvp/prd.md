# PRD: web-dashboard-real-ops-mvp

## Background

The backend already exposes real operator surfaces for status, readiness,
route dry-run, fetcher status, refresh operations, and score explanations. The
web dashboard still contains demo-only behavior, stale MCP tool definitions,
and pages that do not expose the newer operational APIs.

This task turns the existing dashboard into a truthful MVP operator console.
It does not add new backend capabilities unless a small type/contract fix is
needed to consume an existing endpoint safely.

## Goal

Operators can open the web dashboard and inspect or trigger the main existing
operational workflows without seeing simulated data presented as production
state.

## Requirements

### F1: Status Overview

- Dashboard overview uses real `/api/status` data for version, git hash,
  uptime, Redis readiness, WARP summary, xray summary, and proxy counts.
- Dashboard checks `/api/readyz` and displays dependency readiness distinctly
  from process liveness.
- Loading, empty, and error states are visible and do not masquerade as healthy
  data.

### F2: Score-Aware Proxy List

- Proxies page can show score explanation from `/api/proxies/scores`.
- The table exposes score, retention decision, and the strongest explanatory
  components without replacing the existing proxy browsing workflow.
- Score loading failures are visible to the user.

### F3: Route Dry-Run Panel

- Routes page includes a dry-run form for host and protocol.
- It calls `/api/routes/test` and displays matched rule/group, selected exit,
  candidates, unavailable exits, and GeoIP contribution when present.
- Bad input and API errors are handled clearly.

### F4: Fetcher Operations View

- The dashboard has a real fetcher operations view backed by `/api/fetchers`.
- Operators can refresh one fetcher through `/api/fetchers/{id}/refresh` and
  see the returned counts/status.
- Fetcher table shows status, fetched/parsed counts, duration, timestamps, and
  error message if present.

### F5: MCP Debug Surface Accuracy

- MCP Debug tool definitions match currently exposed backend MCP tools.
- REST-backed tools call the matching REST API where one exists.
- Tools that only exist on MCP transport say so explicitly and are not
  presented as successfully executed mock work.

### F6: No Simulated Logs As Real State

- Logs page no longer emits generated demo log lines.
- If no real log stream exists yet, the page must say that the log stream is
  unavailable and point users to real status/fetcher/route views instead.

## Non-Goals

- No direct SSH or host Docker access for dev validation.
- No new real-time log streaming backend in this task.
- No auth, multi-user permissions, or management API token design.
- No new validator observability fields; that remains `validator-observability-v2`.
- No WARP optimizer/pinning enhancements.

## Acceptance Criteria

- [ ] Web Dashboard no longer displays simulated operational data as real
      status.
- [ ] Dashboard, Proxies, Routes, Fetchers, MCP Debug, and Logs main paths use
      real API data or truthful unavailable states.
- [ ] Cross-layer response types live in `web/src/types` and API helpers live
      in `web/src/api` instead of repeating raw JSON assumptions in each view.
- [ ] `npm run build` passes.
- [ ] Existing relevant Rust API tests still pass if backend files are touched.
- [ ] `docs/ROADMAP.md` reflects the task status before final commit.
- [ ] Any dev smoke after push follows `docs/dev-validation.md` and does not
      SSH to the dev address.
