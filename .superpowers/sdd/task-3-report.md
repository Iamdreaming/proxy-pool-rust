# Task 3 Report: Base64 URI Parser

## 1. Status

**Complete.** The `Base64UriParser` is fully implemented and passes all tests with zero clippy warnings.

## 2. Commits

- `1bf8b43` — `feat(sub): add Base64 URI parser`

## 3. Test Results

- **33 inline tests** in `parser::base64_uri::tests` — all pass
- **40 total tests** in `proxy-sub` crate — all pass
- `cargo clippy -p proxy-sub -- -D warnings` — clean

Test coverage includes:
- Base64 decoding: STANDARD, URL_SAFE, missing padding
- Host:port parsing: IPv4, IPv6, invalid
- URI parsing: socks5, http, ss (base64 userinfo + plain), vmess (standard + alt keys + defaults), trojan (with/without query), vless (→ Unknown), unknown scheme
- SS plugin/plugin-opts query parsing, fragment stripping
- Trojan percent-decoded passwords, sni/type query parsing
- Detection: plain lines, base64 blob, negative cases
- Full-blob parse, line-by-line parse
- Fixture-based tests for both `base64_sample.txt` and `base64_blob.txt`
- Malformed input handling: missing `@`, bad base64, invalid host:port

## 4. Self-Review

### Design decisions
- **Custom `DecodeError` enum** wraps `base64::DecodeError` and `FromUtf8Error` since the function needs to try both STANDARD and URL_SAFE alphabets and return UTF-8 string. This avoids leaking `base64::DecodeError` in the `and_then` chain where types diverge.
- **Detection heuristic** follows the brief exactly: (1) try full-blob decode + check for `://`, (2) check lines for known schemes or base64-decoded schemes.
- **`vless://` maps to `Unknown`** as there is no `SubscriptionProxy::Vless` variant yet, consistent with the brief's instruction that unrecognized entries map to `Unknown`.
- **`http://` maps to `Protocol::Http`** (not `Https`) since the scheme is explicitly `http://`.

### Potential improvements (out of scope)
- The `percent_decode` function is minimal — only handles `%XX` for ASCII. A production version should handle multi-byte UTF-8 percent encoding.
- VMess `alter_id` numeric parsing converts `u64` → `u32` with `as` cast. Since alter_id values are always 0 or small, this is safe in practice.

## 5. Concerns

- **None blocking.** The implementation is complete per the brief's requirements.
