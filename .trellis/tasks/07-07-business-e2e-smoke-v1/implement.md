# Implementation Plan: Business Availability E2E Smoke

## Checklist

1. Extend `proxy-core::validator` target input.
   - Add a backwards-compatible target representation for legacy strings and
     structured `{ url, expected_statuses }` objects.
   - Convert matrix targets into `ValidationTarget` before validation.
   - Add unit tests for legacy strings, structured targets, invalid URLs, and
     expected status preservation.

2. Extend REST/MCP matrix input.
   - REST already deserializes `ProxyCheckMatrixRequest`; update tests for both
     legacy and structured targets.
   - Update `proxy-mcp::CheckProxyMatrixParam` to accept the same target shape.
   - Keep MCP error shape unchanged for invalid input.

3. Add business smoke runner.
   - Create `tests/integration/business_e2e_smoke.py`.
   - Reuse `tests/integration/config.py` for API and gateway targets.
   - Implement default target set: Cloudflare trace, GitHub, OpenAI API, Reddit.
   - Query top scored candidates from `/api/proxies/scores`.
   - Run gateway checks and API check-matrix candidate checks.
   - Support `--json`, `--candidate-limit`, `--protocol`, `--timeout`, and
     skip flags for gateway or candidate checks.
   - Precheck `/api/status.git_hash` by default and support
     `--skip-version-check` for intentional stale-deployment diagnosis.

4. Add local tests.
   - Create `tests/integration/test_l0_business_e2e_smoke.py`.
   - Cover status classification, target serialization, candidate extraction,
     summary pass/fail logic, and no-mutation endpoint assumptions.

5. Update roadmap/docs.
   - Set `business-e2e-smoke-v1` as current P0.
   - Move metrics and broad contract-only tasks behind business availability.
   - Mention that this task is observational and no-SSH/no-mutation.

## Validation Commands

```bash
python -m py_compile tests\integration\business_e2e_smoke.py tests\integration\test_l0_business_e2e_smoke.py
python -m pytest tests\integration\test_l0_business_e2e_smoke.py -q
python tests\integration\business_e2e_smoke.py --skip-gateway --skip-candidates --json
cargo test -p proxy-core validator
cargo test -p proxy-api proxy_check_matrix
cargo test -p proxy-mcp check_proxy_matrix
```

Optional live smoke after local tests:

```bash
python tests\integration\business_e2e_smoke.py --json
```

## Rollback Points

- If structured target support causes Rust API/MCP churn, keep the runner's
  gateway checks and defer proxy candidate business targets to a follow-up.
- If live public targets are noisy, preserve the runner but lower default
  thresholds; the report remains useful even when not all targets are reachable.
