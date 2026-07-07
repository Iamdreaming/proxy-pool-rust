# Release Status Public Smoke

## Scenario: Lightweight public release-status smoke

### 1. Scope / Trigger

Use this contract when adding or changing the focused smoke that validates the
public no-SSH release status surfaces without restoring the full REST/MCP
contract smoke suite.

This smoke is for release validation shape and readiness only. It must not
repair, refresh, clean, apply, or deploy anything.

### 2. Signatures

Primary local helper tests:

```powershell
python -m pytest tests\integration\test_l0_release_status_public_smoke.py -q
```

Primary live public smoke:

```powershell
python -m pytest tests\integration\test_release_status_public_smoke.py -q
```

Optional target-version check:

```powershell
$env:PROXY_POOL_GIT_HASH = "abcdef1"
python -m pytest tests\integration\test_release_status_public_smoke.py -q
```

### 3. Contracts

Allowed surfaces:

- REST `GET /api/status`
- REST `GET /api/readyz`
- MCP `service_status`
- MCP `update_status`

Forbidden surfaces and actions:

- Direct SSH to the dev address.
- Host Docker CLI/API access.
- MCP `update_service`.
- Pool refresh, proxy deletion, cleanup apply, subscription apply, config apply,
  or any other mutating operation.

Status payload fields asserted by `helpers.release_status`:

- Top-level `version`, `git_hash`, `uptime_sec`
- `release.app_version`, `release.git_hash`, `release.update_enabled`,
  `release.update_container`, `release.configured_image`,
  `release.image_repo`, `release.image_tag`, `release.watchtower_url`
- `pool.http`, `pool.https`, `pool.socks5`, `pool.total`
- `redis.status`
- `quality.total`, `quality.score_buckets`, `quality.recent_samples`,
  `quality.recent_success_rate`, `quality.recent_failures`,
  `quality.stale_proxies`, `quality.stale_after_secs`, `quality.retention`,
  `quality.top_failure_reasons`
- `warp.configured`, `warp.healthy`
- `xray.enabled`, `xray.active_nodes`, `xray.failed_nodes`,
  `xray.removed_nodes`, `xray.total_nodes`

Readiness payload contract:

- HTTP status is `200` or `503`.
- Body contains `status` with `ok` or `error`.
- Optional `message` is a string.

Update-status contract:

- `status` is one of `never_triggered`, `disabled`, `already_current`,
  `updated`, or `failed`.
- `never_triggered` is valid without extra metadata.
- Other statuses include `update_enabled`, `container_name`, `image`,
  `image_repo`, `image_tag`, and `watchtower_url`.

Hash comparison contract:

- When `PROXY_POOL_GIT_HASH` is unset, live public smoke validates shape only.
- When `PROXY_POOL_GIT_HASH` is set, both HTTP and MCP `git_hash` /
  `release.git_hash` must start with the expected value.
- Do not auto-detect local `HEAD` for this focused smoke; local work may be
  ahead of dev and should not make shape validation fail.

### 4. Validation & Error Matrix

| Condition | Expected result |
|-----------|-----------------|
| `/api/status` lacks release metadata | Smoke fails on release contract assertion |
| `/api/status.pool.total` does not equal protocol sum | Smoke fails on pool contract assertion |
| `/api/readyz` returns 503 with structured `{"status":"error"}` | Smoke passes shape check |
| MCP transport unavailable | Live smoke fails on public MCP call |
| MCP `update_status.status=never_triggered` | Smoke passes without requiring update metadata |
| MCP `update_status.status=failed` without a message | Smoke fails |
| Dev is on an older hash and `PROXY_POOL_GIT_HASH` is unset | Smoke passes shape check |
| Dev is on an older hash and `PROXY_POOL_GIT_HASH` is set | Smoke fails hash check |

### 5. Good/Base/Bad Cases

- Good: HTTP status and MCP service status expose matching release metadata,
  readyz returns a structured body, and update_status is a known read-only
  state.
- Base: Dev is healthy but not running local `HEAD`; the smoke passes when no
  explicit expected hash is set.
- Bad: The smoke calls `update_service` or refreshes the pool to fix stale
  state. It must report public state only.

### 6. Tests Required

- Local helper tests cover accepted status payload, hash mismatch, readyz shape,
  known update statuses, and read-only MCP tool constants.
- Live smoke covers `/api/status`, `/api/readyz`, MCP `service_status`, and MCP
  `update_status`.
- Existing broader REST/MCP integration tests should reuse the shared helper for
  release/status fields instead of copying the contract.
- Run `py_compile` for touched Python integration files.

### 7. Wrong vs Correct

#### Wrong

```python
def test_public_status(api_client, expected_git_hash):
    # expected_git_hash auto-detects local HEAD, which may not be deployed.
    assert_status_payload_contract(api_client.get("/api/status").json(), expected_git_hash)
```

This makes a shape smoke fail just because local work is ahead of dev.

#### Correct

```python
from config import EXPECTED_GIT_HASH

def test_public_status(api_client):
    assert_status_payload_contract(api_client.get("/api/status").json(), EXPECTED_GIT_HASH)
```

Only `PROXY_POOL_GIT_HASH` opts into target-version comparison.
