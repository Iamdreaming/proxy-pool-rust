# Task 6: Source Discovery — Discover Trait + StaticUrl + ContentCache + SubscriptionSource

**Files:**
- Replace stub: `crates/proxy-sub/src/discover/mod.rs` (currently just `//! TODO: implement`)
- Create: `crates/proxy-sub/src/discover/static_url.rs`
- Replace stub: `crates/proxy-sub/src/source/mod.rs` (currently just `//! TODO: implement`)
- Create: `crates/proxy-sub/src/source/cache.rs`
- Test: inline tests

**Interfaces:**
- Consumes: nothing from earlier tasks (this is foundational)
- Produces: `Discover` trait, `StaticUrlDiscover`, `ContentCache`, `SubscriptionSource`

## Requirements

### 1. Discover trait (discover/mod.rs)

```rust
#[async_trait::async_trait]
pub trait Discover: Send + Sync {
    fn name(&self) -> &str;
    async fn discover(&self) -> Vec<String>;
}
```

Module structure:
```rust
pub mod aggregator;  // stub for Task 7
pub mod github_search;  // stub for Task 7
pub mod static_url;
```

### 2. StaticUrlDiscover (discover/static_url.rs)

- Takes a `Vec<String>` of URLs from config
- `discover()` simply returns the cloned list
- Inline tests: test with URLs, test with empty list

### 3. ContentCache (source/cache.rs)

- In-memory `HashMap<String, (String, Instant)>` with TTL
- `get(&mut self, url: &str) -> Option<String>` — returns content if not expired, removes expired entries
- `put(&mut self, url: &str, content: &str)` — stores content with current Instant
- `evict_expired(&mut self)` — removes all expired entries
- `len()` and `is_empty()` methods
- Inline tests: put/get, missing key, expired entry, evict_expired

### 4. SubscriptionSource (source/mod.rs)

- Fields: `client: reqwest::Client`, `cache: ContentCache`, `timeout: Duration`
- `new(cache_ttl_sec: u64, timeout_sec: u64)` — builds reqwest client with timeout
- `fetch(&mut self, url: &str) -> Result<String>` — check cache first, else HTTP GET, store in cache
- `evict_expired(&mut self)` — delegate to cache
- Inline tests: test constructor works

## Stubs for Task 7
Create stub files for:
- `crates/proxy-sub/src/discover/github_search.rs`: `//! TODO: implement`
- `crates/proxy-sub/src/discover/aggregator.rs`: `//! TODO: implement`

## Global Constraints
- Same as previous tasks
- Commit: `feat(sub): add Discover trait, StaticUrlDiscover, ContentCache, SubscriptionSource`
