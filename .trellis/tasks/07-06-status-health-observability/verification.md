# Verification: status-health-observability

## Local Verification

- `cargo fmt --all`
- `cargo fmt --all -- --check`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `npx vue-tsc --noEmit` from `web/`
- `npm run build` from `web/`
- `docker compose -f deploy/docker-compose.yml config`
- `git diff --check`

## Notes

- `npm run build` passed with the existing Vite large chunk warning.
- `git diff --check` reported only Windows LF-to-CRLF normalization warnings.
- Deployed-instance pytest suites were not run against this working tree because
  `tests/integration/config.py` targets `100.64.0.2` by default. They should be
  run after this change is deployed, or against a local instance configured with
  `PROXY_POOL_HOST` and related environment variables.
