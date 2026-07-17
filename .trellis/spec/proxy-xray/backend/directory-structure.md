# Directory Structure — proxy-xray

> How backend code is organized in the proxy-xray crate.

---

## Overview

proxy-xray is a library crate that provides xray-core integration. It has no binary entry point — it is assembled and wired by `proxy-server`. The crate is organized around distinct responsibilities: config generation, subprocess management, gRPC client, port allocation, and background sync.

---

## Directory Layout

```
crates/proxy-xray/
├── Cargo.toml              # Dependencies: tonic, prost, tokio, serde_json, thiserror, anyhow
├── build.rs                # tonic-build: compiles xray protobuf files
├── protos/                 # xray-core protobuf definitions
│   └── xray/
│       ├── app/proxyman/command/   # HandlerService gRPC API
│       ├── common/                 # net, protocol, serial, geodata
│       ├── core/                   # core config
│       ├── proxy/                  # shadowsocks, vmess, trojan, socks, freedom
│       └── transport/internet/     # tcp, tls, websocket, grpc
└── src/
    ├── lib.rs              # Module declarations
    ├── models.rs           # XrayNode, SyncStats
    ├── config_gen.rs       # ConfigGenerator — JSON config generation
    ├── port_manager.rs     # PortManager — local SOCKS5 port allocation
    ├── process.rs          # XrayProcess — subprocess lifecycle + supervision
    ├── xray_client.rs      # XrayClient — gRPC + CLI hybrid client
    ├── outbound_sync.rs    # OutboundSync — pending→active sync loop
    └── proto.rs            # Generated protobuf module re-exports
```

---

## Module Organization

| Module | Responsibility | Key Types | Dependencies |
|--------|---------------|-----------|-------------|
| `models` | Data structures for xray node representation and sync statistics | `XrayNode`, `SyncStats` | `serde` |
| `config_gen` | Generate xray JSON configs (inbound/outbound/routing) for SS/VMess/Trojan | `ConfigGenerator`, `XrayNodeConfig` | `proxy-sub` (SubscriptionProxy), `serde_json` |
| `port_manager` | Allocate/release/claim local SOCKS5 ports in a configurable range | `PortManager` | `tokio` (RwLock) |
| `process` | Start/stop/restart xray-core subprocess with supervision | `XrayProcess` | `tokio` (process, watch) |
| `xray_client` | gRPC + CLI hybrid client for xray HandlerService | `XrayClient` | `proto` (tonic), `tokio` (process, watch, RwLock) |
| `outbound_sync` | Background loop: active revalidate/demote → pending admission → stale remove | `OutboundSync`, `XrayValidationPlan`, `TeardownKind` | All above + `proxy-core` (ProxyStore, Proxy, Validator, XrayStatusRegistry), `proxy-sub` (PendingStore) |
| `proto` | Generated protobuf/gRPC code from xray proto files | `HandlerServiceClient`, request/response types | `tonic`, `prost` |

### Dependency Flow

```
outbound_sync
  ├── config_gen (generates JSON configs)
  ├── models (XrayNode, SyncStats)
  ├── port_manager (allocates ports)
  ├── xray_client (pushes configs to xray)
  │     └── proto (gRPC types)
  ├── proxy-core (ProxyStore, Proxy, XraySettings)
  └── proxy-sub (PendingStore, SubscriptionProxy)
```

---

## Naming Conventions

- **File names**: `snake_case`, one primary struct per file (e.g., `port_manager.rs` → `PortManager`)
- **Struct names**: `PascalCase` matching file name (e.g., `XrayProcess`, `XrayClient`, `OutboundSync`)
- **Tag format**: `"{protocol_label}-{host}-{port}"` (e.g., `"ss-1.2.3.4-8388"`)
- **Inbound tags**: `"in-{tag}"` (e.g., `"in-ss-1.2.3.4-8388"`)
- **Outbound tags**: `"out-{tag}"` (e.g., `"out-ss-1.2.3.4-8388"`)
- **Protocol labels**: `"ss"`, `"vmess"`, `"trojan"` — must match `SubscriptionProxy::protocol_label()`
- **Proto module hierarchy**: Mirrors protobuf package paths (`xray.app.proxyman.command`, `xray.common.net`, etc.)

---

## Adding New Encrypted Protocol Support

To add a new encrypted protocol (e.g., VLESS):

1. **`protos/`**: Add the protocol's `.proto` file under `xray/proxy/<protocol>/`
2. **`build.rs`**: Add the proto file path to the `proto_files` array
3. **`proto.rs`**: Add the module re-export under `pub mod proxy`
4. **`config_gen.rs`**: Add a new arm in `generate_outbound_json()` for the `SubscriptionProxy` variant
5. **`outbound_sync.rs`**: Add the protocol label to the `labels` array in `sync_once()`
6. **`models.rs`**: No changes needed — `protocol_label` is a string field

---

## Examples

- **Config generation**: `config_gen.rs` is the canonical example of protocol-specific JSON generation with the `SubscriptionProxy` match pattern
- **Supervision loop**: `process.rs::XrayProcess::supervise()` demonstrates the `tokio::select!` + exponential backoff pattern
- **Watch channel consumer**: `outbound_sync.rs::OutboundSync::run()` shows how to react to connection state changes via `connected_rx.changed()`
