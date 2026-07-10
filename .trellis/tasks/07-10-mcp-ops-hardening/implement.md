# Implement: MCP ops hardening

Validation: `cargo test -p proxy-mcp`; final `cargo test` + `cargo clippy
--workspace --all-targets -- -D warnings`.

## Phase 1 — async update_service
1. [ ] Add `UpdateStatusKind::InProgress` + `UpdateStatusSnapshot::in_progress`.
2. [ ] Extract `run_update(Arc<RwLock<UpdateStatusSnapshot>>, UpdateServiceConfig)`
       from the current update_service body (write terminal snapshot to the Arc).
3. [ ] Rewrite `update_service(&self)`: sync guards → record `in_progress` →
       `tokio::spawn(timeout(300s, run_update))` (record `failed` on elapse) →
       return `{"status":"update_started"}`.
4. [ ] Unit tests: in_progress snapshot shape; terminal transitions.

## Phase 2 — container_logs tool
5. [ ] `docker_api_get_raw(socket, path)` — GET, split headers, read body to EOF;
       `#[cfg(not(unix))]` stub error.
6. [ ] `demux_docker_log_stream(&[u8]) -> String` pure helper + fallback.
7. [ ] `ContainerLogsParam` + `container_logs` tool (defaults, clamp, error map).
8. [ ] Unit tests: demux synthetic 2-frame buffer; raw fallback; tail clamp.

## Phase 3 — verify + ship
9. [ ] full cargo test + clippy green.
10. [ ] trellis-check; commit; push; watch CI.
11. [ ] deploy (update_service now works from split MCP, or compose pull/up);
        verify update_service returns immediately + update_status progresses;
        container_logs returns proxy-pool lines.

## Rollback
- Both features are additive to proxy-mcp; revert the commit to restore the
  synchronous update_service and drop the logs tool.
