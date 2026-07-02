# Quality Guidelines

Coding standards and conventions for `proxy-server`.

---

## Architecture Rules

### Single-File Wiring

`main.rs` is **wiring only**. It must not contain business logic. Valid activities:
- Building components from other crates
- Creating `Arc` references and channels
- Spawning service tasks
- Conditional feature setup

Invalid activities (move to the appropriate crate):
- Proxy validation logic → `proxy-core::validator`
- HTTP request handling → `proxy-api::routes`
- Protocol parsing → `proxy-gateway`

### Dependency Direction

`proxy-server` depends on **all** other crates. No other crate may depend on it.

```
proxy-server → proxy-core, proxy-api, proxy-gateway, proxy-mcp, proxy-sub, proxy-xray
```

If two crates need to share a type, put it in `proxy-core`.

---

## Arc and Channel Conventions

### Arc Sharing

All cross-service state is shared via `Arc`. The pattern is:

```rust
let store = Arc::new(ProxyStore::new(/* ... */));
// Pass Arc clones to each consumer:
let api_state = AppState { store: store.clone(), /* ... */ };
let selector = Arc::new(UpstreamSelector::new(store.clone(), /* ... */));
```

**Rule**: Clone `Arc` before passing into `tokio::spawn`. The clone is cheap
(reference count increment only).

### Channel Patterns

Two channel patterns are used:

1. **mpsc + oneshot (command/reply)** — `SchedulerHandle`:
   ```rust
   let (cmd_tx, cmd_rx) = mpsc::channel::<SchedulerCommand>(8);
   let handle = SchedulerHandle::new(cmd_tx);
   // cmd_rx passed to Scheduler::run()
   ```

2. **watch (state broadcast)** — `XrayClient` connection state:
   ```rust
   let (connected_tx, connected_rx) = watch::channel(false);
   // connected_rx cloned and passed to OutboundSync
   ```

When adding new cross-service communication, prefer these patterns. Do not
use `std::sync::Mutex` across await points — use `tokio::sync::Mutex` instead.

---

## Conditional Features

The only conditional feature is `settings.xray.enabled`. The pattern:

```rust
let xray_handle: Option<JoinHandle<()>>;
if settings.xray.enabled {
    // ... setup ...
    xray_handle = Some(tokio::spawn(/* ... */));
} else {
    tracing::info!("xray integration disabled (set xray.enabled=true to enable)");
    xray_handle = None;
}
```

In `tokio::select!`, handle `None` with `std::future::pending()`:

```rust
_r = async {
    if let Some(h) = xray_handle { h.await }
    else { std::future::pending().await }
} => tracing::info!("xray stopped"),
```

---

## Forbidden Patterns

| Pattern | Why | Alternative |
|---------|-----|-------------|
| Business logic in `main.rs` | Violates single-responsibility; hard to test | Move to appropriate crate |
| `std::sync::Mutex` across `.await` | Deadlock risk | Use `tokio::sync::Mutex` or `RwLock` |
| Hardcoded ports/URLs | Inflexible, breaks in different environments | Use `Settings` config with defaults |
| Multiple Redis connections for the same purpose | Connection waste | Share via `Arc<MultiplexedConnection>` clone |
| `tokio::spawn(async { panic!(...) })` | Silent panic, no logging | Use `unwrap_or_else` with `tracing::error!` |
| Ignoring `JoinHandle` results | Misses panic propagation | At minimum, log the result in `tokio::select!` |

---

## Testing Conventions

`proxy-server` has **no unit tests** — it's pure wiring. Integration testing
should be done by:

1. Starting the server with a test config and Redis instance.
2. Exercising the API and gateway endpoints.
3. Verifying pool state via API responses.

The other crates (proxy-core, proxy-api, etc.) own their own unit tests.

---

## Adding a New Top-Level Service

Checklist:

1. [ ] Add the crate to `Cargo.toml` `[dependencies]`
2. [ ] Import and build the component in `main.rs`
3. [ ] Clone any needed `Arc` references before spawning
4. [ ] `tokio::spawn` the service's async run loop
5. [ ] Add a `tokio::select!` branch for the new handle
6. [ ] Add config fields to `Settings` (in `proxy-core::config`) with defaults
7. [ ] Document the new service in this spec's Service Composition table
