# Config Runbook Drift Check

## Scenario: Dev update config and runbook drift guard

### 1. Scope / Trigger

Use this contract when changing any of these surfaces:

- `deploy/docker-compose.yml` update env wiring.
- `docs/dev-validation.md` operator validation guidance.
- README text that summarizes dev validation.
- `/api/status.release` or MCP `service_status.release` field names used by
  release validation.
- MCP `update_status` / `update_service` env names or Watchtower wiring.

The goal is to keep dev validation no-SSH, read-only by default, and aligned
with the actual local source contract.

### 2. Signatures

Primary local check:

```powershell
python -m pytest tests\integration\test_l0_config_runbook_drift.py -q
```

The check reads repository files only. It must not open network connections,
call Docker, call MCP, or require Redis/proxy-pool to be running.

Relevant files:

- `deploy/docker-compose.yml`
- `docs/dev-validation.md`
- `README.md`
- `crates/proxy-core/src/status.rs`

### 3. Contracts

App container update env contract:

- `PROXY_POOL_UPDATE_ENABLED=true`
- `PROXY_POOL_UPDATE_DOCKER_SOCKET=/var/run/docker.sock`
- `PROXY_POOL_UPDATE_CONTAINER=proxy-pool`
- `PROXY_POOL_UPDATE_IMAGE=ghcr.io/iamdreaming/proxy-pool-rust:latest`
- `PROXY_POOL_UPDATE_WATCHTOWER_URL=http://watchtower-proxy-pool:8080/v1/update`
- `PROXY_POOL_UPDATE_TOKEN=${PROXY_POOL_UPDATE_TOKEN:-proxy-pool-update}`

Watchtower token contract:

- `WATCHTOWER_HTTP_API_TOKEN=${PROXY_POOL_UPDATE_TOKEN:-proxy-pool-update}`
- The app token and Watchtower token must be documented as a matched pair.

Release metadata field contract:

- `release.git_hash`
- `release.configured_image`
- `release.update_enabled`
- `release.update_container`
- `release.image_repo`
- `release.image_tag`
- `release.watchtower_url`

Operator boundary contract:

- Direct SSH to the dev address is not part of default validation.
- Host Docker CLI/API access is not part of default validation.
- MCP `update_service` is a mutating operator action, not a status check.
- `containrrr/watchtower` may lack common shell tools such as `printenv`; do
  not recommend `docker compose exec watchtower-proxy-pool printenv` as the
  token verification path.

### 4. Validation & Error Matrix

| Condition | Expected test result |
|-----------|----------------------|
| Compose omits a required `PROXY_POOL_UPDATE_*` env | Drift test fails on compose env assertion |
| Watchtower token no longer derives from `PROXY_POOL_UPDATE_TOKEN` | Drift test fails on token wiring assertion |
| Runbook omits a required update env | Drift test fails on runbook env assertion |
| Runbook documents obsolete `release.update_image` | Drift test fails on operator-doc obsolete field assertion |
| Status source renames/removes a release field without docs update | Drift test fails on status source or runbook field assertion |
| Runbook recommends Watchtower `printenv` as the token check | Drift test fails until guidance points to compose/app env/status instead |
| Runbook weakens no-SSH or no-routine-update boundary | Drift test fails on boundary assertions |

### 5. Good/Base/Bad Cases

- Good: Compose, docs, README, and status source all describe
  `configured_image`, image repo/tag, update envs, and no-SSH validation
  consistently.
- Base: The runbook points to compose and public read-only status surfaces for
  verification, while live dev may still be behind the latest image.
- Bad: A stale runbook tells operators to look for `release.update_image` or to
  exec into Watchtower with `printenv`; this creates false triage work and may
  violate the no-SSH workflow.

### 6. Tests Required

When changing this area, keep or update assertions for:

- Compose app update env entries.
- Watchtower token wiring.
- Runbook env and token-pair documentation.
- Runbook release field names.
- Absence of obsolete operator field names such as `release.update_image`.
- No-SSH, no host-Docker, and no routine `update_service` wording.
- Watchtower shell-tool limitation and recommended alternatives.

### 7. Wrong vs Correct

#### Wrong

```markdown
MCP `service_status` should expose `release.update_image`.
Verify the Watchtower token with:
docker compose exec watchtower-proxy-pool printenv WATCHTOWER_HTTP_API_TOKEN
```

This documents a field that does not exist and relies on a shell utility that
the Watchtower image may not provide.

#### Correct

```markdown
MCP `service_status` should expose `release.configured_image`,
`release.image_repo`, and `release.image_tag`.
Verify update wiring through compose, the app-container update env,
`service_status.release`, or MCP `update_status`.
```
