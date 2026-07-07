# PRD: pool-quality-metrics-v1

## Background

`proxy-quality-history-lite` already stores bounded recent validation history
inside each proxy and exposes per-proxy score explanations. Operators still need
a no-SSH, read-only way to judge the overall pool trend from `/api/status`,
MCP `service_status`, and `/api/metrics` without querying individual proxies or
touching mutating update tools.

## Goal

Expose an aggregate, low-cardinality proxy quality summary through the shared
service status model and Prometheus metrics.

## Non-Goals

- Do not resume `revalidation-scheduler-priority-v1`.
- Do not resume `fetcher-source-quality-ranking`.
- Do not add cleanup, downgrade, refresh, or update actions.
- Do not expose proxy hosts, ports, full URLs, subscription content, or raw
  free-form error strings as Prometheus labels.
- Do not build or modify Dashboard UI in this task.

## Requirements

### F1: Shared Quality Summary

`proxy-core` owns the quality aggregation. REST API and MCP consumers must
serialize the shared `ServiceStatus` shape instead of recomputing quality fields
locally.

The summary must include:

- total proxies scanned for quality aggregation
- score bucket counts
- recent sample count
- aggregate recent success rate, nullable when no recent samples exist
- recent failure count
- stale proxy count and the threshold used to classify stale proxies
- retention-risk counts for below-min-score and hard-failure candidates
- normalized top recent failure reasons

### F2: Status Surface

`/api/status` and MCP `service_status` must include the same `quality` object
because both use the shared service status model.

### F3: Prometheus Surface

`/api/metrics` must expose low-cardinality quality metrics derived from the same
status snapshot. Labels are allowed only for bounded enum-like dimensions such as
score bucket, retention decision, and normalized failure reason.

### F4: Backward-Compatible Failure Behavior

If Redis quality collection fails, the service must still return a status
snapshot with default quality values and `redis.status="error"`. The failure
must not panic and must not trigger any mutating operation.

## Acceptance Criteria

- [x] `ServiceStatus` includes a serialized `quality` object with the fields
      listed above.
- [x] `/api/status` integration smoke asserts the quality shape.
- [x] MCP `service_status` integration smoke asserts the same quality shape.
- [x] `/api/metrics` includes quality score buckets, stale proxy count, recent
      sample/failure totals, recent success rate, and retention candidate
      metrics.
- [x] Prometheus failure-reason labels are normalized to a bounded set and never
      use raw proxy addresses, full URLs, or raw untrusted error strings.
- [x] Empty-pool and no-recent-sample cases serialize deterministically.
- [x] No direct SSH, host Docker access, or `update_service` call is used for
      validation.
- [x] `cargo fmt --all --check` passes.
- [x] `cargo test -p proxy-core` passes.
- [x] `cargo test -p proxy-api` passes.
- [x] `cargo test -p proxy-mcp` passes.
- [x] `cargo test --workspace --all-targets` passes.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` passes.
