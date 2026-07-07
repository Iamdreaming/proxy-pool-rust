# Business Availability Smoke

## Scenario: Gateway and proxy-candidate business reachability

### 1. Scope / Trigger

Use this contract when adding or changing `tests/integration/business_e2e_smoke.py`
or tests that validate real target reachability for business sites.

This smoke is for operational diagnosis. It must not repair, refresh, clean,
delete, update, apply, or reconfigure anything.

### 2. Signatures

Primary command:

```powershell
python tests\integration\business_e2e_smoke.py --json
```

Useful variants:

```powershell
python tests\integration\business_e2e_smoke.py --protocol http --candidate-limit 5
python tests\integration\business_e2e_smoke.py --expected-git-hash 3d69e0b
python tests\integration\business_e2e_smoke.py --skip-version-check
python tests\integration\business_e2e_smoke.py --skip-gateway
python tests\integration\business_e2e_smoke.py --skip-candidates
```

Supported environment variables come from `tests/integration/config.py`:

- `PROXY_POOL_HOST`
- `PROXY_POOL_API_PORT`
- `PROXY_POOL_GW_PORT`
- `PROXY_POOL_GIT_HASH`

### 3. Contracts

Allowed public surfaces:

- REST `GET /api/status` for runtime version precheck.
- Gateway as an HTTP CONNECT proxy through `PROXY_POOL_GW_PORT`.
- REST `GET /api/proxies/scores`.
- REST `POST /api/proxy/check-matrix`.

Forbidden surfaces and actions:

- Direct SSH to the dev address.
- Host Docker CLI/API access.
- MCP `update_service`.
- Pool refresh, fetcher refresh, proxy deletion, cleanup apply, subscription
  apply, config apply, or any other mutating operation.

Default business targets:

| Target | URL | Success rule |
|--------|-----|--------------|
| Cloudflare trace | `https://www.cloudflare.com/cdn-cgi/trace` | Any 2xx/3xx |
| GitHub | `https://github.com/` | Any 2xx/3xx |
| OpenAI API | `https://api.openai.com/v1/models` | `401` |
| Reddit | `https://www.reddit.com/` | `200`, `403`, or `429` |

OpenAI and Reddit use explicit expected statuses because an auth failure or
rate-limit response can prove that the network path reached the target.

### 4. Validation & Error Matrix

| Condition | Expected result |
|-----------|-----------------|
| Runtime git hash matches expected hash | Continue with gateway and candidate checks |
| Runtime git hash does not match expected hash | Overall result fails and the runner exits before gateway/candidate checks |
| Gateway target returns accepted status | Gateway check is `ok` with status and elapsed time |
| Gateway target returns unexpected status | Gateway check fails with route/fallback triage hint |
| Gateway target raises timeout/connect error | Gateway check fails with exception type and target details |
| Candidate list is empty for a protocol | Protocol source result fails with pool/source-quality hint; overall candidate component fails only when no candidate reaches a business target |
| Candidate matrix has at least one alive target | Candidate check is `ok` and includes server diagnostics |
| Candidate matrix reaches no target | Candidate check fails but preserves per-target errors |
| `--skip-gateway` is set | Gateway component does not affect overall result |
| `--skip-candidates` is set | Candidate component does not affect overall result |

### 5. Good/Base/Bad Cases

- Good: Gateway reaches at least one business target and at least one stored
  candidate reaches at least one business target, even if another protocol has
  no candidates.
- Base: One target fails but other targets provide usable evidence, and the
  runtime git hash proves the deployment is the intended version; the report
  keeps every per-target result visible.
- Bad: The runner calls `refresh_pool` to create candidates or `update_service`
  to repair deployment state.

### 6. Tests Required

- Local pure tests for target status classification and matrix target
  serialization.
- Local pure tests for candidate extraction and summary pass/fail logic.
- Static or unit coverage proving the runner only references observational API
  endpoints.
- `python -m py_compile` for the runner and tests.
- Optional live smoke after local tests pass; live failures are useful evidence
  when they report current public-surface state.

### 7. Wrong vs Correct

#### Wrong

```powershell
python tests\integration\business_e2e_smoke.py
# script sees no candidates and calls refresh_pool automatically
```

This turns diagnosis into mutation and can hide why the business path was
unusable.

#### Correct

```powershell
python tests\integration\business_e2e_smoke.py --json
```

The runner reports gateway and candidate reachability. Operators decide
separately whether refresh, update, or route changes are appropriate.
