# Implementation Plan: CI/CD + MCP Self-Update Stabilization

## Phase 1: Planning And Baseline

- [x] Inspect current task artifacts and repo state.
- [x] Confirm existing status/version/health code already landed.
- [x] Create design and implementation artifacts for the remaining stabilization scope.

## Phase 2: MCP Update Safety

- [x] Add an `UpdateServiceConfig` helper in `proxy-mcp`.
- [x] Load update config from environment inside `update_service`.
- [x] Return `disabled` before touching Docker when `PROXY_POOL_UPDATE_ENABLED` is false.
- [x] Replace hard-coded image, container name, socket path, Watchtower URL, and token.
- [x] Return old image ID, new image ID/digest, `digest_changed`, and update trigger result.
- [x] Add unit tests for config parsing and image identity comparison.

## Phase 3: Runtime Stability

- [x] Replace subscription Redis startup panic with logged non-fatal degradation.
- [x] Replace API bind/serve unwraps with explicit error logging.
- [x] Keep API/Gateway/Scheduler fatal behavior visible through the main `select!`.

## Phase 4: Deployment Docs And Roadmap

- [x] Wire update environment variables in `deploy/docker-compose.yml`.
- [x] Update `docs/ROADMAP.md` to reflect already-landed update work and remaining external deploy verification.
- [x] Keep claims conservative where remote push/deploy has not been verified in this session.

## Phase 5: Verification

- [x] `cargo test --workspace --all-targets`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `npm run build` from `web/`
- [x] Note that integration tests require a deployed instance and are not run locally unless the service target is available.

## Phase 6: Dev Update Verification

- [x] Push `fced1d2` and `665e200` to `origin/main`.
- [x] Verify GitHub Actions build-and-push success for run `28807329192`.
- [x] Verify GitHub Actions build-and-push success for run `28807505058`.
- [x] Trigger dev update through MCP `update_service`; the first response was interrupted because Watchtower recreated the container.
- [x] Verify dev `/api/status.git_hash` updated from `a2436f1` to `665e200`.
- [x] Sync `PROXY_POOL_UPDATE_*` runtime env through the approved compose path.
- [x] Verify MCP `update_service` returns `already_current` after env sync.
- [ ] Deferred by user: invalid image/token/Watchtower path should leave the old container running.

## Phase 7: Backlog Planning

- [x] Defer the update failure-injection check into the roadmap as `update-failure-hardening`.
- [x] Re-plan the next TODO list around proxy input quality after confirming status/health observability is already implemented.

## Risk Points

- `update_service` can affect a live container when enabled; the safety switch and env wiring must be reviewed carefully.
- Watchtower may kill the current process before the MCP HTTP response reaches the caller; final success must be verified through `/api/status.git_hash`.
- Docker API behavior can differ by daemon version; parsing must stay defensive.
- Direct SSH to the dev service address is not allowed; dev runtime changes must go through an approved deployment path.
