# Task 9: Refresh Loop + Main Integration

**Files:**
- Replace stub: `crates/proxy-sub/src/refresh.rs`
- Modify: `crates/proxy-server/Cargo.toml` (add proxy-sub dep)
- Modify: `Cargo.toml` (add proxy-sub to workspace deps)
- Modify: `crates/proxy-server/src/main.rs` (add subscription loop spawn)
- Test: inline tests in refresh.rs

**Interfaces:**
- Consumes: `SubscriptionConfig` (Task 8), `Discover` trait (Task 6), `SubscriptionSource` (Task 6), `parse_subscription()` (Task 2), `partition()` (Task 8), `ProxyStore` (proxy-core), `PendingStore` (Task 8)
- Produces: `subscription_refresh_loop()`, `build_discoverers()`

## Requirements

### 1. refresh.rs

**Key functions:**

`subscription_refresh_loop()`: Runs forever with interval sleep. Each cycle calls `run_refresh_cycle()`.

`run_refresh_cycle()`:
1. Call all discoverers → collect URL list
2. Dedup URLs
3. Evict expired cache entries
4. For each URL: fetch → parse_subscription → partition → store basic in ProxyStore → store encrypted in PendingStore
5. Log summary (total_basic, total_encrypted, failed_urls)

`build_discoverers(config: &SubscriptionConfig) -> Vec<Arc<dyn Discover>>`:
1. If `config.urls` is non-empty → create StaticUrlDiscover
2. If `config.github.enabled` → create GitHubSearchDiscover (use default keywords "clash free sub" and "v2ray free nodes" if keywords is empty)
3. For each aggregator config → create AggregatorDiscover

**Inline tests:**
- `test_build_discoverers_static`: config with 1 URL → 1 discoverer
- `test_build_discoverers_all`: config with all 3 types → 3 discoverers

### 2. proxy-server/Cargo.toml

Add:
```toml
proxy-sub = { path = "../proxy-sub" }
```

### 3. Workspace Cargo.toml

Add to `[workspace.dependencies]`:
```toml
proxy-sub = { path = "crates/proxy-sub" }
```

Then in proxy-server/Cargo.toml use `proxy-sub = { workspace = true }`.

### 4. main.rs integration

Add at top of main.rs:
```rust
use proxy_sub::refresh::{build_discoverers, subscription_refresh_loop};
use proxy_sub::pending::PendingStore;
use proxy_sub::source::SubscriptionSource;
```

After `scheduler_handle` block (~line 102), add:
```rust
let sub_handle = {
    let sub_config = settings.subscription.clone();
    let discoverers = build_discoverers(&sub_config);
    let sub_source = SubscriptionSource::new(sub_config.cache_ttl_sec, sub_config.fetch_timeout_sec);
    let pending = Arc::new(PendingStore::new(redis_multiplexed));
    tokio::spawn(subscription_refresh_loop(
        sub_config,
        discoverers,
        sub_source,
        store.clone(),
        pending,
    ))
};
```

Add to `tokio::select!`:
```rust
r = sub_handle => tracing::info!("subscription refresh stopped: {:?}", r),
```

## IMPORTANT: redis_multiplexed ownership

Note that in main.rs, `redis_multiplexed` is used to create `ProxyStore` in Task 1 (line 46-47). Since `MultiplexedConnection` is `Clone`, you can clone it before passing to `PendingStore::new()`.

Look at how `store` is created: `ProxyStore::new(redis_multiplexed, ...)` — the `redis_multiplexed` is moved there. You need to clone it BEFORE it's moved. Check the current code flow and adjust accordingly. The `MultiplexedConnection` implements `Clone` so `redis_multiplexed.clone()` works.

## Global Constraints
- Same as previous tasks
- Commit: `feat(sub): add refresh loop and main integration`
