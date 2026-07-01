# Task 4: V2Ray JSON Parser

**Files:**
- Replace stub: `crates/proxy-sub/src/parser/v2ray_json.rs`
- Create: `crates/proxy-sub/tests/fixtures/v2ray_sample.json`
- Test: inline tests

**Interfaces:**
- Consumes: `Parser` trait (Task 2), `SubscriptionProxy` (Task 1)
- Produces: `V2rayJsonParser`

## Requirements

Implement `V2rayJsonParser` that:
1. **detect()**: Checks if content is valid JSON containing `outbounds` key
2. **parse()**: Extracts outbounds array, converts each outbound to SubscriptionProxy

### Supported outbound protocols
- `socks` → Basic (Protocol::Socks5)
- `http` → Basic (Protocol::Http)
- `vmess` → Vmess variant
- `shadowsocks` → Shadowsocks variant
- `trojan` → Trojan variant
- `vless` → Unknown (Phase 2)

### JSON structure
```json
{
  "outbounds": [
    {
      "protocol": "socks",
      "settings": { "servers": [{ "address": "host", "port": 1080 }] },
      "tag": "name"
    },
    {
      "protocol": "vmess",
      "settings": { "vnext": [{ "address": "host", "port": 443, "users": [{ "id": "uuid", "alterId": 0, "security": "auto" }] }] },
      "streamSettings": { "network": "ws", "wsSettings": { "path": "/v2", "headers": { "Host": "host" } }, "security": "tls", "tlsSettings": { "serverName": "sni" } },
      "tag": "name"
    },
    {
      "protocol": "trojan",
      "settings": { "servers": [{ "address": "host", "port": 443, "password": "pass" }] },
      "streamSettings": { "network": "tcp", "security": "tls", "tlsSettings": { "serverName": "sni" } },
      "tag": "name"
    },
    {
      "protocol": "shadowsocks",
      "settings": { "servers": [{ "address": "host", "port": 8388, "method": "aes-256-gcm", "password": "pass" }] },
      "tag": "name"
    }
  ]
}
```

### streamSettings extraction
- `network` field → network (default "tcp")
- `wsSettings.path` → path
- `wsSettings.headers.Host` → host_header
- `tlsSettings.serverName` → sni
- `grpcSettings.serviceName` → path (for gRPC transport)

### Test fixture
Create `crates/proxy-sub/tests/fixtures/v2ray_sample.json` with 4 outbounds (socks, vmess, trojan, shadowsocks).

## Global Constraints
- Same as previous tasks
- Commit: `feat(sub): add V2Ray JSON parser`
