# GitHub Airport Source Candidates

## Purpose

Identify practical source shapes for public GitHub "airport collection" ingestion and classify them by risk before implementation.

## Candidate Source Shapes

### Curated raw subscription URL

Examples include repositories that publish a generated Clash/V2Ray subscription file at a stable raw URL.

Pros:
- Simple fit for `subscription.urls`.
- Low implementation cost.
- Easy to preview/apply one source at a time.

Risks:
- A single upstream can disappear, change format, or publish low-quality nodes.
- License and provenance may be unclear.

### Aggregator URL list

Examples include repositories that publish text/JSON/YAML lists of subscription URLs.

Pros:
- Fits existing `subscription.aggregators`.
- Gives broader coverage without using GitHub Search API.
- Operator can preview aggregator-level discovered URL and parse counts before applying.

Risks:
- Higher blast radius if the list includes low-quality or private-looking sources.
- Needs conservative documentation and redacted operator output.

### GitHub Search discovery

Uses existing `subscription.github` with keywords such as `clash free sub` and `v2ray free nodes`.

Pros:
- Dynamically discovers recently updated sources.
- Already implemented as `GitHubSearchDiscover`.

Risks:
- Highest noise and provenance risk.
- Code search API rate limits apply, especially without a token.
- Search can return unrelated, malicious, or token-bearing content.

## Recommendation

Use a curated static/aggregator pack for the first implementation. Keep GitHub Search documented as an opt-in advanced mode rather than enabling it as part of the default pack.

This matches the existing project shape and keeps the first rollout reversible: the operator can remove or comment out a source entry and stop ingestion without code changes.

## Source Examples To Verify During Design

- `https://github.com/wzdnzd/aggregator`
- `https://github.com/Au1rxx/free-vpn-subscriptions`
- `https://github.com/xiaoji235/airport-free`
- Search keywords: `clash free sub`, `v2ray free nodes`, `免费 机场 订阅 clash`

These examples are research inputs, not automatically trusted production defaults.
