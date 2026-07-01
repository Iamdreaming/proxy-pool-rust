# Task 8: Config Extension + Proxy Conversion + Pending Storage

**Files:**
- Modify: `crates/proxy-core/src/config.rs` (add SubscriptionConfig, GitHubDiscoverConfig, AggregatorEntryConfig)
- Replace stub: `crates/proxy-sub/src/convert.rs` (currently `//! TODO: implement`)
- Replace stub: `crates/proxy-sub/src/pending.rs` (currently `//! TODO: implement`)
- Test: inline tests

**Interfaces:**
- Consumes: `SubscriptionProxy` (Task 1), `Proxy`/`Protocol` (proxy-core), `redis` (proxy-core)
- Produces: `SubscriptionConfig`, `GitHubDiscoverConfig`, `AggregatorEntryConfig`, `to_proxy()`, `partition()`, `PendingStore`

## Requirements

### 1. Add SubscriptionConfig to proxy-core/src/config.rs

Add these new structs:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubDiscoverConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default = "default_github_max")]
    pub max_results: u32,
    #[serde(default = "default_github_interval")]
    pub search_interval_sec: u64,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatorEntryConfig {
    pub url: String,
    #[serde(default = "default_agg_format")]
    pub format: String,
    #[serde(default = "default_agg_interval")]
    pub refresh_interval_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionConfig {
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub github: GitHubDiscoverConfig,
    #[serde(default)]
    pub aggregators: Vec<AggregatorEntryConfig>,
    #[serde(default = "default_sub_interval")]
    pub refresh_interval_sec: u64,
    #[serde(default = "default_sub_timeout")]
    pub fetch_timeout_sec: u64,
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_sec: u64,
}
```

Add `subscription: SubscriptionConfig` field to the `Settings` struct.

Add default value functions:
```rust
fn default_github_max() -> u32 { 20 }
fn default_github_interval() -> u64 { 86400 }
fn default_agg_format() -> String { "text".into() }
fn default_agg_interval() -> u64 { 43200 }
fn default_sub_interval() -> u64 { 3600 }
fn default_sub_timeout() -> u64 { 30 }
fn default_cache_ttl() -> u64 { 1800 }
```

Add Default impls for `GitHubDiscoverConfig` and `SubscriptionConfig`.

### 2. Proxy Conversion (convert.rs)

```rust
pub fn to_proxy(sub: &SubscriptionProxy, source_url: &str) -> Option<Proxy>
pub fn partition(subs: &[SubscriptionProxy], source_url: &str) -> (Vec<Proxy>, Vec<SubscriptionProxy>)
```

- `to_proxy()`: Only converts `SubscriptionProxy::Basic` → `Proxy`. Sets `proxy.source = Some(format!("subscription:{source_url}"))`. Returns `None` for non-basic variants.
- `partition()`: Splits a slice into (basic_proxies, encrypted_subs).

Inline tests:
- `test_to_proxy_basic`: verify Basic conversion
- `test_to_proxy_encrypted_returns_none`: verify SS returns None
- `test_partition`: verify split of mixed list

### 3. Pending Storage (pending.rs)

```rust
pub struct PendingStore {
    conn: MultiplexedConnection,
}
```

Methods:
- `new(conn: MultiplexedConnection)`
- `store_batch(&self, nodes: &[SubscriptionProxy]) -> Result<()>`: Store in Redis ZSet `pending:encrypted:{protocol_label}`, score = Unix timestamp, member = JSON-serialized SubscriptionProxy
- `get_pending(&self, protocol_label: &str, limit: usize) -> Result<Vec<SubscriptionProxy>>`: Read from ZSet
- `count_pending(&self, protocol_label: &str) -> Result<usize>`: Count ZSet members

Inline tests:
- `test_subscription_proxy_serialization_roundtrip`: verify JSON serialize/deserialize roundtrip

## Global Constraints
- Same as previous tasks
- Commit: `feat(sub): add SubscriptionConfig, proxy conversion, and pending storage`
