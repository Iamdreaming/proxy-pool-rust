# Task 8: Config Extension + Proxy Conversion + Pending Storage

## Status: DONE

## Commits
- `1384b7f` feat(sub): add SubscriptionConfig, proxy conversion, and pending storage

## Test Results
- proxy-core: 2/2 passed
- proxy-sub: 106/106 passed (including 4 new tests)
- clippy: 0 warnings on both crates

### New Tests
| Test | File | Result |
|------|------|--------|
| `test_to_proxy_basic` | convert.rs | PASS |
| `test_to_proxy_encrypted_returns_none` | convert.rs | PASS |
| `test_partition` | convert.rs | PASS |
| `test_subscription_proxy_serialization_roundtrip` | pending.rs | PASS |

## Self-Review

### config.rs additions
- Added `GitHubDiscoverConfig`, `AggregatorEntryConfig`, `SubscriptionConfig` structs after `FreePoolSettings` (as specified).
- Added `subscription: SubscriptionConfig` field to `Settings` struct with `#[serde(default)]`.
- Added 7 default value functions (`default_github_max`, `default_github_interval`, `default_agg_format`, `default_agg_interval`, `default_sub_interval`, `default_sub_timeout`, `default_cache_ttl`) alongside existing ones.
- Added `Default` impls for `GitHubDiscoverConfig` and `SubscriptionConfig` using the `serde_yaml::from_str("{}").unwrap()` pattern.
- No `AggregatorEntryConfig::Default` because `url` is a required field (no `#[serde(default)]`), consistent with `WarpInstanceConfig` which also has required fields and no Default impl.

### convert.rs
- `to_proxy()`: Only converts `Basic` variant; sets `source = Some(format!("subscription:{source_url}"))`. Returns `None` for all other variants.
- `partition()`: Splits a slice into `(Vec<Proxy>, Vec<SubscriptionProxy>)` — basics converted, encrypted cloned.
- Moved `use proxy_core::models::Protocol` into the `#[cfg(test)]` module to avoid unused-import warning in production code.
- Tests cover: basic conversion, encrypted returning None (SS + VMess), and partition of a mixed list (2 basic + 2 encrypted).

### pending.rs
- `PendingStore` holds `MultiplexedConnection` directly (cheaply cloneable, per brief).
- Uses `self.conn.clone()` for each async operation (same pattern as `ProxyStore`).
- `store_batch()`: Uses `chrono::Utc::now().timestamp()` as ZSet score, JSON-serialized `SubscriptionProxy` as member.
- `get_pending()`: Reads from ZSet with `zrevrange` (most recent first).
- `count_pending()`: Uses `zcard`.
- Redis key format: `pending:encrypted:{protocol_label}` (as specified).
- Serialization roundtrip test covers both `Shadowsocks` and `Basic` variants.

## Concerns
- `PendingStore` methods are all async and require a live Redis connection — no integration tests added (only unit-level serialization roundtrip). Integration tests would need a Redis mock or test container, which is out of scope for this task.
- `AggregatorEntryConfig` has a required `url` field with no Default impl, matching the pattern of `WarpInstanceConfig` (also no Default for required fields). This means `Settings::default()` will still work because `SubscriptionConfig::default()` produces an empty `aggregators` vec.
