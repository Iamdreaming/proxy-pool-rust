# Integration Testing Guidelines

> Local and live integration helpers for validating deployed proxy-pool-rust
> instances through public, documented surfaces.

## Overview

Integration tooling under `tests/integration/` should validate deployed service
behavior without taking ownership of runtime state. Dev validation defaults to
GitHub Actions, public HTTP endpoints, MCP read-only tools, and pytest fixtures
that use `PROXY_POOL_*` environment variables.

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Read-only Dev Smoke Runner](./readonly-dev-smoke-runner.md) | Command contract for post-push no-SSH validation | Done |
| [Config Runbook Drift Check](./config-runbook-drift-check.md) | Local contract for keeping dev compose, runbook docs, and release metadata fields aligned | Done |
| [Release Status Public Smoke](./release-status-public-smoke.md) | Lightweight public HTTP/MCP release-status smoke contract | Done |
| [Business Availability Smoke](./business-availability-smoke.md) | No-mutation gateway and proxy-candidate smoke for real target reachability | Done |

## Pre-Development Checklist

- [ ] Read `docs/dev-validation.md` before adding or changing deployment
      validation helpers.
- [ ] Read `config-runbook-drift-check.md` before changing dev compose update
      env wiring, release metadata field names, or operator runbook text.
- [ ] Reuse `tests/integration/config.py` for `PROXY_POOL_*` environment
      variables.
- [ ] Reuse `tests/integration/helpers/mcp_client.py` for MCP Streamable HTTP
      calls.
- [ ] Keep default validation read-only: no SSH, no host Docker, no mutating MCP
      tools.
- [ ] Business target checks must report per-target status and error details
      instead of hiding partial failures behind one aggregate result.

## Quality Check

- [ ] Local tests or `py_compile` validate helper logic without requiring live
      dev access.
- [ ] Live smoke failures produce actionable public-surface triage hints.
- [ ] New helper commands document how to run from the repository root.
- [ ] Compose/runbook/release-field changes keep
      `tests/integration/test_l0_config_runbook_drift.py` passing.
- [ ] `.codex/config.toml` and other unrelated local config files are not staged
      with integration helper changes.
