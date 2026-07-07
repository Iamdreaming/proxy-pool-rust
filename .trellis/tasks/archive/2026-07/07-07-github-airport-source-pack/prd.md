# GitHub Airport Source Pack

## Goal

Safely bring public GitHub proxy subscription collections into this project as optional, operator-controlled subscription sources.

The user value is to increase the available proxy/node pool by using both curated public sources and GitHub discovery without turning untrusted GitHub content into an always-on trusted feed.

## Confirmed Facts

- The project already has subscription discovery and parsing under `crates/proxy-sub`.
- `SubscriptionConfig` supports static URLs, GitHub search, and aggregator entries through `subscription.urls`, `subscription.github`, and `subscription.aggregators`.
- Operator preview/apply paths already exist through API and MCP:
  - `GET /api/subscriptions/sources`
  - `POST /api/subscriptions/sources/{id}/refresh`
  - MCP `subscription_sources`
  - MCP `refresh_subscription_source`
- Existing parsers cover Clash YAML, base64 URI subscriptions, V2Ray JSON, and Surge.
- Existing encrypted-node activation covers `ss`, `vmess`, and `trojan` through Redis pending storage and xray sync.
- Existing pool quality controls include validation, latency/anonymity detection, weighted score ranking, `pool.min_score`, retention decisions, quality summaries, `/api/proxies/scores`, MCP `explain_proxy_scores`, and dry-run-first `cleanup_low_score_proxies`.
- Existing subscription preview reports already expose source-level evidence: discovered URL count, unique URL count, fetched/failed URL count, parsed node count, direct/encrypted/unknown node count, duplicate node count, protocol counts, sanitized errors, and outcome.
- Many public GitHub collections may include unsupported protocols such as `vless`, `hysteria2`, `tuic`, `ssr`, or `snell`; these should not block safe ingestion, but they should be visible in preview reports.

## Requirements

- R1: Add an optional hybrid GitHub airport source pack that combines curated sources with controlled GitHub Search discovery.
- R2: The curated pack must use existing subscription mechanisms where possible instead of introducing a separate ingestion pipeline.
- R3: The pack must default to safe operation: discover/preview first, apply only through existing explicit apply paths or an explicitly enabled refresh loop.
- R4: Public sources must be auditable: source labels should be visible and query tokens/fragments should remain redacted in operator-facing output.
- R5: The implementation must avoid hardcoding private subscription links, secrets, or account-specific tokens.
- R6: The source pack should distinguish between:
  - static raw subscription URLs,
  - aggregator list URLs,
  - GitHub Search keywords.
- R7: Unsupported or unknown protocols must be counted/reportable and skipped safely rather than causing refresh failure.
- R8: Documentation must tell an operator how to enable the curated pack, preview a source, apply a source, and roll back by disabling/removing the source entries.
- R9: GitHub Search results must be treated as candidate discovery, not as a trusted curated source. Candidate sources should be filtered through existing preview, validation, score, and retention signals before operators rely on them.
- R10: Preview must produce an operator-friendly apply recommendation so humans do not need to manually inspect raw node lists. The recommendation should classify each source as `apply`, `review`, or `reject` with short reasons derived from source-level metrics.
- R11: The recommendation should use pre-apply evidence from preview, while documentation must make clear that post-apply quality is still governed by validation, score ranking, retention, xray status, and cleanup dry-runs.

## Acceptance Criteria

- [ ] Configuration or documented preset exists for at least one hybrid GitHub airport source pack.
- [ ] The preset can be enabled without editing Rust source code.
- [ ] Preview refresh shows discovered URL count, parsed node count, protocol counts, duplicate counts, unknown node count, and errors without writing to the pool.
- [ ] Preview refresh includes an apply recommendation (`apply`, `review`, or `reject`), a numeric/source-level confidence score or grade, and human-readable reasons.
- [ ] Apply refresh writes supported basic nodes and encrypted `ss`/`vmess`/`trojan` nodes through the existing store/pending flow.
- [ ] GitHub Search can be enabled as a bounded candidate source with explicit keywords and result limits.
- [ ] Documentation explains that scoring and cleanup evaluate nodes after ingestion, while preview/source statistics evaluate source quality before apply.
- [ ] Documentation gives concrete guidance for apply decisions, including recommended thresholds for fetch success rate, supported protocol ratio, unknown node ratio, duplicate ratio, parsed node yield, and error count.
- [ ] Source labels and errors do not leak query tokens or URL fragments in API/MCP output.
- [ ] Documentation includes recommended safe rollout steps:
  1. enable one source,
  2. preview,
  3. inspect protocol and error counts,
  4. apply,
  5. check pool/xray status,
  6. disable on poor quality.
- [ ] Tests or validation cover the source-pack config shape and preview/apply behavior at the level appropriate to the final implementation.

## Out Of Scope

- Building or hosting a new crawler service.
- Adding paid/private subscription accounts.
- Automatically trusting every GitHub Search result.
- Default-enabling public airport sources in production config.
- Full support for every protocol found in public collections unless explicitly added during design review.

## Product Decision

- The implementation should combine A and B:
  - curated static/aggregator sources as the recommended stable lane;
  - GitHub Search as a bounded candidate-discovery lane;
  - preview-based source recommendations as the pre-apply gate;
  - existing validation, scoring, retention, and cleanup controls as the post-apply quality gate after nodes are ingested or activated.
- A `reject` preview recommendation blocks normal `apply=true` by default. A future explicit override such as `force=true` may be added later if operators need to accept known-risk sources.
- First-version recommendation thresholds:
  - `apply`: fetched URL success rate >= 60%, parsed nodes >= 20, supported protocol ratio >= 50%, unknown node ratio <= 40%, duplicate node ratio <= 70%, and no dominant fetch/parse error pattern.
  - `review`: some usable parsed nodes exist, but one or more `apply` thresholds are weak or the source is noisy.
  - `reject`: zero usable nodes, fetches almost all fail, supported protocol ratio is extremely low, unknown nodes dominate, duplicate ratio is extreme, or errors indicate the source is malformed/private/unusable.

## Open Questions

- None currently blocking planning.
