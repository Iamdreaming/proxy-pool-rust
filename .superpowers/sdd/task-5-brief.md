# Task 5: Surge Parser

**Files:**
- Replace stub: `crates/proxy-sub/src/parser/surge.rs`
- Create: `crates/proxy-sub/tests/fixtures/surge_sample.txt`
- Create: `crates/proxy-sub/tests/fixtures/mixed_invalid.txt`
- Test: inline tests in surge.rs + tests in parser/mod.rs

**Interfaces:**
- Consumes: `Parser` trait (Task 2), `SubscriptionProxy` (Task 1)
- Produces: `SurgeParser`

## Requirements

Implement `SurgeParser` that:
1. **detect()**: Checks if lines match Surge pattern: `Name = type, server, port, [params...]`
2. **parse()**: Parses each line into `SubscriptionProxy`

### Surge line format
```
proxy-name = type, server, port, key1=value1, key2=value2, ...
```

Supported types and their params:
- `socks5`: just server, port → Basic(Socks5)
- `http`: server, port → Basic(Http)
- `ss`: server, port, encrypt-method=xx, password=xx → Shadowsocks
- `vmess`: server, port, username=uuid, [tls=true], [ws=true], [ws-path=/x], [ws-host=host], [sni=sni], [network=tcp] → Vmess
- `trojan`: server, port, password=xx, sni=xx → Trojan

### Test fixtures

**surge_sample.txt**:
```
socks5-proxy = socks5, 10.0.0.1, 1080
http-proxy = http, 10.0.0.2, 8080
ss-proxy = ss, 10.0.0.3, 8388, encrypt-method=aes-256-gcm, password=mypassword
vmess-proxy = vmess, 10.0.0.4, 443, username=a3482e88-686a-4a58-8126-99c9df64b7bf, tls=true, ws=true, ws-path=/v2ray, ws-host=vmess.example.com, sni=vmess.example.com
trojan-proxy = trojan, 10.0.0.5, 443, password=trojanpass, sni=trojan.example.com
```

**mixed_invalid.txt**:
```
this is not a proxy config
random text here
no valid format detected
```

### Additional tests in parser/mod.rs
Add these tests to verify `parse_subscription()` works correctly with Surge format and rejects invalid content:
1. `test_parse_subscription_surge`: parse Surge content, verify 2 basic proxies
2. `test_parse_subscription_no_match_fixture`: parse mixed_invalid.txt, verify empty result

## Global Constraints
- Same as previous tasks
- Commit: `feat(sub): add Surge parser and test fixtures`
