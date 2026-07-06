# Design: subscription source ops MVP

## Shape

Add a small operations layer inside `proxy-sub` that can run a refresh cycle with
structured reporting. The existing parser, discoverers, `SubscriptionSource`,
`ProxyStore`, and `PendingStore` stay as the execution primitives; the new layer
records what happened and exposes it safely to API/MCP.

Proposed public models in `proxy-sub`:

- `SubscriptionSourceKind`: `static_url`, `github_search`, `aggregator`.
- `SubscriptionSourceDescriptor`: stable id, kind, label/url, enabled flag.
- `SubscriptionRefreshMode`: `Preview` or `Apply`.
- `SubscriptionSourceReport`: discovered/fetched URL counts, parsed counts by
  protocol, stored basic/encrypted counts, unknown/duplicate counts, errors,
  elapsed time, and timestamp.
- `SubscriptionOpsState`: shared in-memory status registry containing configured
  descriptors and latest report per source.

## Data Flow

`proxy-server` builds `SubscriptionOpsState` from `settings.subscription` once at
startup and passes it to:

- the background subscription refresh loop, which updates reports after each
  cycle while preserving existing write behavior;
- `proxy-api::AppState`, which exposes REST status and refresh endpoints;
- `ProxyPoolMcpConfig`, which exposes equivalent MCP tools.

Manual refresh uses the same configured source descriptors. It resolves the
requested id, runs only that source, and returns a report. The default mode is
preview, so the path fetches/parses/partitions but does not write to Redis. When
`apply=true`, it stores direct nodes into `ProxyStore` and encrypted nodes into
`PendingStore`.

## API/MCP

REST endpoints:

- `GET /api/subscriptions/sources`: returns descriptors and latest reports.
- `POST /api/subscriptions/sources/{id}/refresh?apply=false`: manual
  preview/apply for one source.

MCP tools:

- `subscription_sources`: status equivalent to the GET endpoint.
- `refresh_subscription_source`: manual preview/apply by id.

## Error Handling

Discovery/fetch/parse/store failures are captured per source and per URL. A
manual refresh for an unknown source id returns `404` in REST and a structured
MCP error JSON. API/MCP responses never include raw subscription content or full
node credentials; counts and source URLs are enough for this MVP.

## Testing

Unit tests cover descriptor generation, report aggregation, dry-run not writing,
and serialization contract. API/MCP unit tests cover response shape and default
dry-run parameters. Integration smoke tests assert REST/MCP route/tool presence
and basic empty-state behavior.
