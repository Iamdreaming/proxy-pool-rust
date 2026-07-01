# Proxy Subscription Source & Parser Design (Phase 1)

**Date**: 2026-07-01
**Branch**: feat/proxy-chain-warp
**Phase**: Phase 1 — subscription parsing & source discovery

## Problem

The proxy pool currently has only 5 text-based fetcher sources (ProxyScrape, TheSpeedX, FreeProxyList, Clarketm, GeoNode), all returning plain `host:port` lists. These cover at most a few hundred proxies. The real volume is in **subscription links** — Clash YAML, V2Ray base64, and other formats shared on GitHub, Telegram, and aggregator sites. A single subscription can contain hundreds to thousands of nodes.

Expanding the fetcher layer to support subscriptions can increase pool size from hundreds to thousands, and eventually tens of thousands with xray integration (Phase 2).

## Scope

**Phase 1** covers:
- New `proxy-sub` crate: subscription format parsing (4 formats) + source discovery (3 methods)
- Integration with existing ProxyStore for basic-protocol nodes (socks5/http/https)
- Encrypted-protocol nodes (ss/vmess/trojan) stored as pending in Redis for Phase 2
- Configuration extension and scheduled task integration

**Phase 2** (future, not implemented here):
- `proxy-xray` crate: xray-core single-instance + gRPC API hot-reload
- Local socks5 port allocation per encrypted node
- UpstreamSelector `Xray` branch

## Architecture: Method C — Progressive Split

### New crate: `proxy-sub`

```
crates/proxy-sub/
  Cargo.toml
  src/
    lib.rs               # pub mod aggregation
    models.rs            # SubscriptionProxy enum
    parser/              # Format parsing engine
      mod.rs             # Parser trait + parse_subscription() entrypoint
      clash.rs           # Clash YAML → Vec<SubscriptionProxy>
      base64_uri.rs      # Base64 decode → URI list → Vec<SubscriptionProxy>
      v2ray_json.rs      # V2Ray JSON outbounds → Vec<SubscriptionProxy>
      surge.rs           # Surge line format → Vec<SubscriptionProxy>
    discover/            # Source discovery
      mod.rs             # Discover trait + build_discoverers()
      static_url.rs      # Static URLs from config
      github_search.rs   # GitHub Search API discovery
      aggregator.rs      # Known aggregator projects
    source/              # Subscription fetch + cache
      mod.rs             # SubscriptionSource: URL → raw content
      cache.rs           # Local content cache (avoid duplicate fetches)
```

## Core Models

### `SubscriptionProxy`

Defined in `proxy-sub/src/models.rs`:

```rust
/// A proxy node parsed from a subscription, carrying full protocol parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubscriptionProxy {
    Basic {
        host: String,
        port: u16,
        protocol: Protocol,  // socks5/http/https
    },
    Shadowsocks {
        host: String,
        port: u16,
        method: String,
        password: String,
        plugin: Option<String>,
        plugin_opts: Option<String>,
    },
    Vmess {
        host: String,
        port: u16,
        uuid: String,
        alter_id: u32,
        security: String,
        network: String,       // tcp/ws/grpc/etc
        path: Option<String>,
        host_header: Option<String>,
        sni: Option<String>,
    },
    Trojan {
        host: String,
        port: u16,
        password: String,
        sni: Option<String>,
        network: Option<String>,
    },
    Unknown {
        raw_config: String,
    },
}

impl SubscriptionProxy {
    pub fn is_direct_usable(&self) -> bool {
        matches!(self, Self::Basic { .. })
    }

    pub fn source(&self) -> Option<&str> { ... }  // tracking where it came from
}
```

### `EncryptedProxyState` (Phase 2 reservation)

Defined in `proxy-core/src/models.rs` for forward compatibility:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum EncryptedProxyState {
    Pending,
    Active { local_socks5_port: u16 },
    Failed,
}
```

### `Upstream` extension point (Phase 2)

In `proxy-gateway/src/upstream.rs`:

```rust
pub enum Upstream {
    Direct,
    Proxy(Proxy),
    Warp { socks5_port: u16 },
    // Phase 2: Xray { local_socks5_port: u16 },
    NoProxy,
}
```

## Parser Trait & Format Detection

```rust
#[async_trait]
pub trait Parser: Send + Sync {
    fn name(&self) -> &str;
    /// Check whether raw content matches this format.
    fn detect(&self, content: &str) -> bool;
    /// Parse content into a list of subscription proxies.
    fn parse(&self, content: &str) -> Vec<SubscriptionProxy>;
}
```

`parse_subscription()` entrypoint in `parser/mod.rs`:
- Iterates through registered parsers, calls `detect()` on each
- First match wins, calls `parse()` on the matching parser
- If no parser matches, logs a warning and returns empty vec

### Format detection heuristics

| Format | Detection Signal |
|--------|-----------------|
| Clash YAML | `proxies:` key present, valid YAML |
| Base64 URI | Base64-decodable, decoded content contains `://` (vmess://, ss://, trojan://) |
| V2Ray JSON | `outbounds` key present, valid JSON |
| Surge | Lines matching `^\w+\s*=\s*(ss|vmess|trojan|http|socks5)\s*,` pattern |

### Clash YAML parser detail

Extract the `proxies:` array from Clash YAML. Each entry has:
- `type`: ss/vmess/trojan/socks5/http → maps to `SubscriptionProxy` variant
- `server`, `port`: host and port
- Protocol-specific fields extracted per type

Clash proxy types not in our enum (e.g., `hysteria2`, `wireguard`) map to `Unknown`.

### Base64 URI parser detail

1. Base64-decode the entire content
2. Split decoded text by newline
3. Each line is a protocol URI: `ss://base64@host:port`, `vmess://base64_json`, `trojan://password@host:port`, `socks5://host:port`
4. Parse each URI according to its scheme

### V2Ray JSON parser detail

1. Parse JSON, find `outbounds` array
2. For each outbound: extract `protocol` (vmess/vless/ss/trojan/socks/http)
3. Extract `settings.vnext[0]` (host, port, users) and `streamSettings` (network, security, tlsSettings)

### Surge parser detail

1. Split by newline
2. Each line: `Name = type, server, port, [params...]`
3. Parse according to type keyword

## Source Discovery

### Discover trait

```rust
#[async_trait]
pub trait Discover: Send + Sync {
    fn name(&self) -> &str;
    /// Discover subscription URL list.
    async fn discover(&self) -> Vec<String>;
}
```

### StaticUrlDiscover

Simplest discoverer: reads pre-configured URL list from `SubscriptionConfig.urls`.

```yaml
subscription:
  urls:
    - https://raw.githubusercontent.com/xxx/clash.yaml
    - https://example.com/sub?token=abc
```

### GitHubSearchDiscover

Uses GitHub Search API to find recently updated subscription repos/code.

**Search strategy**:
1. **Repository search**: query keywords (`clash free sub`, `v2ray free nodes`), sorted by recently updated, limit to `max_results`
2. **Code search**: search for `.yaml` files containing `proxies:` keyword in code, extract raw URLs
3. For each discovered repo: scan for files named `*.yaml`, `*.yml`, `*.txt` with proxy content
4. Convert GitHub file paths to raw URLs: `https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{path}`

```yaml
subscription:
  github:
    enabled: true
    token: ghp_xxx                  # GitHub PAT to avoid rate limit
    max_results: 20
    search_interval_sec: 86400      # Search once per day
    keywords:
      - "clash free sub"
      - "v2ray free nodes"
```

Rate limit handling: 429 → exponential backoff, 403 → skip and log warning.

### AggregatorDiscover

Connects to known aggregator projects that collect subscription URLs.

```yaml
subscription:
  aggregators:
    - url: https://raw.githubusercontent.com/aggregator/list.txt
      format: text                  # text = one URL per line
      refresh_interval_sec: 43200
```

Format types:
- `text`: one subscription URL per line
- `json`: JSON array of `{url, format}` objects
- `yaml`: YAML with `subscriptions:` list

## Subscription Source Fetch & Cache

### SubscriptionSource

```rust
pub struct SubscriptionSource {
    client: reqwest::Client,
    cache: ContentCache,
    timeout: Duration,
}

impl SubscriptionSource {
    /// Fetch subscription content from URL, with caching.
    pub async fn fetch(&mut self, url: &str) -> Result<String> {
        if let Some(content) = self.cache.get(url) {
            return Ok(content);
        }
        let resp = self.client.get(url).timeout(self.timeout).send().await?;
        let content = resp.text().await?;
        self.cache.put(url, &content);
        Ok(content)
    }
}
```

### ContentCache

- In-memory HashMap<String, (String, Instant)> with TTL
- TTL configurable via `SubscriptionConfig.cache_ttl_sec`
- Eviction: lazy check on `get()` — expired entries removed

## Integration with Existing System

### Configuration extension

In `proxy-core/src/config.rs`, add `SubscriptionConfig` to `Settings`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    // ... existing fields ...
    #[serde(default)]
    pub subscription: SubscriptionConfig,
}
```

Default values:
- `refresh_interval_sec`: 3600 (1 hour)
- `fetch_timeout_sec`: 30
- `cache_ttl_sec`: 1800 (30 minutes)

### Scheduled task

In `proxy-server/src/main.rs`, add a new tokio spawn alongside existing fetch/validate loops:

```rust
tokio::spawn(subscription_refresh_loop(
    sub_config,
    discoverers,
    source,
    store,
    redis_conn,
));
```

Loop logic:
1. Call all discoverers → collect URL list
2. Dedup URLs (remove already-fetched within cache TTL)
3. For each URL: fetch content → `parse_subscription()` → partition into basic vs encrypted
4. Basic nodes: convert to `Proxy` → `store.add()` (with dedup)
5. Encrypted nodes: serialize to JSON → Redis `ZADD pending:encrypted:{protocol}` with timestamp score

### Proxy conversion

`SubscriptionProxy::Basic` → `Proxy` mapping:

```rust
fn to_proxy(sub: &SubscriptionProxy, source: &str) -> Option<Proxy> {
    match sub {
        SubscriptionProxy::Basic { host, port, protocol } => {
            let mut proxy = Proxy::new(host.clone(), *port, *protocol);
            proxy.source = Some(source.to_string());
            Some(proxy)
        }
        _ => None,
    }
}
```

### Redis pending storage for encrypted nodes

Key format: `pending:encrypted:ss`, `pending:encrypted:vmess`, `pending:encrypted:trojan`

Score: Unix timestamp of discovery time (enables time-range queries)

Member: JSON-serialized `SubscriptionProxy`

Phase 2's `proxy-xray` will:
1. Read pending nodes from these keys
2. Generate xray outbound config per node
3. Push config via xray gRPC API
4. Allocate local socks5 port
5. Create `Proxy` entry with `EncryptedProxyState::Active { local_socks5_port }`

## Dependencies — `proxy-sub/Cargo.toml`

```toml
[package]
name = "proxy-sub"
edition = "2024"

[dependencies]
proxy-core = { path = "../proxy-core" }
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
serde_json = "1"
base64 = "0.22"
reqwest = { version = "0.12", features = ["rustls-tls"] }
async-trait = "0.1"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
anyhow = "1"
chrono = { version = "0.4", features = ["serde"] }
redis = { version = "0.27", features = ["tokio-comp"] }
url = "2"

[dev-dependencies]
tokio = { version = "1", features = ["test-util", "macros"] }
```

## Testing Strategy

### Unit tests

- Each parser: test `detect()` with valid/invalid/edge-case content
- Each parser: test `parse()` with known fixtures (Clash YAML, base64, V2Ray JSON, Surge)
- SubscriptionProxy: test `is_direct_usable()` for each variant

### Integration tests

- `proxy-sub/tests/`: test full pipeline (discover → fetch → parse → partition)
- Mock HTTP responses for source fetch
- Mock GitHub API responses for search discovery

### Test fixtures

Store sample subscription files in `proxy-sub/tests/fixtures/`:
- `clash_sample.yaml`
- `base64_sample.txt`
- `v2ray_sample.json`
- `surge_sample.txt`
- `mixed_invalid.txt` (content that no parser should match)

## Error Handling

- Parser errors: log warning, skip malformed entries, continue parsing remaining entries
- Source fetch errors: log warning, return empty content (don't crash the refresh loop)
- GitHub API rate limit: exponential backoff, skip on 403
- Redis connection errors: log error, skip encrypted node storage, continue with basic nodes
- Dedup: use `Proxy::dedup_key()` for basic nodes, `SubscriptionProxy` host+port+type for encrypted nodes

## Success Criteria

1. All 4 format parsers pass unit tests with real fixture data
2. StaticUrlDiscover + GitHubSearchDiscover + AggregatorDiscover all functional
3. Subscription refresh loop runs without crashing alongside existing fetch/validate loops
4. Basic-protocol nodes from subscriptions appear in ProxyStore and are selectable by UpstreamSelector
5. Encrypted-protocol nodes stored in Redis pending keys, queryable by protocol type
6. Pool size increases measurably after first subscription refresh cycle

## Out of Scope (Phase 2)

- xray-core process management
- gRPC API client for xray config hot-reload
- Local socks5 port allocation for encrypted nodes
- UpstreamSelector `Xray` branch
- IP quality/blacklist detection
- FOFA/Shodan fetcher
- FreePool round-robin strategy improvement
