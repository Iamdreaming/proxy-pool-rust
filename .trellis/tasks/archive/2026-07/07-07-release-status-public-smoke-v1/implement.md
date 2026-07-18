# Implementation Plan

## Checklist

- [x] Review existing integration status tests and helper utilities.
- [x] Add a focused public release status smoke test file.
- [x] Factor small local assertion helpers only if needed to avoid duplicating
      the same release/status shape checks in one file.
- [x] Add local tests or static assertions proving the smoke tool list is
      read-only.
- [x] Run targeted pytest for the new smoke.
- [x] Run py_compile for touched integration Python files.
- [x] Optionally run the new smoke against live dev through public HTTP/MCP only.

## Validation Commands

- `python -m py_compile tests\integration\helpers\release_status.py tests\integration\test_l0_release_status_public_smoke.py tests\integration\test_release_status_public_smoke.py tests\integration\test_l2_api.py tests\integration\test_l4_mcp.py`
- `python -m pytest tests\integration\test_l0_release_status_public_smoke.py tests\integration\test_release_status_public_smoke.py -q`
- `python -m pytest tests\integration\test_l2_api.py::TestApiStatus::test_status_returns_version tests\integration\test_l4_mcp.py::TestMcpServiceStatus::test_service_status_structure tests\integration\test_l4_mcp.py::TestMcpServiceStatus::test_update_status_read_only_structure -q`

## Notes

- Keep this task read-only by default.
- If live dev is stale or unavailable, report that public-surface result instead
  of attempting repair.
- The live smoke intentionally only compares runtime git hash when
  `PROXY_POOL_GIT_HASH` is set. Without that env var, it validates public shape
  only, so local work ahead of dev does not fail the focused smoke.
