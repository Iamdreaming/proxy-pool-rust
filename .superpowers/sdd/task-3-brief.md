# Task 3: Base64 URI Parser

**Files:**
- Replace stub: `crates/proxy-sub/src/parser/base64_uri.rs`
- Create: `crates/proxy-sub/tests/fixtures/base64_sample.txt`
- Create: `crates/proxy-sub/tests/fixtures/base64_blob.txt`
- Test: inline tests in base64_uri.rs

**Interfaces:**
- Consumes: `Parser` trait (Task 2, in `crate::parser`), `SubscriptionProxy` (Task 1, in `crate::models`)
- Produces: `Base64UriParser`

## Requirements

Implement `Base64UriParser` that handles:
1. **Full blob**: entire content is one base64 string → decode → split by newline → parse URIs
2. **Individual lines**: each line is base64-encoded or already a plain URI
3. **URI schemes**: `ss://`, `vmess://`, `trojan://`, `socks5://`, `http://`, `vless://`

### Detection heuristic
- Try decoding entire content as base64 → check if decoded text contains `://`
- OR check if lines start with known URI schemes (`ss://`, `vmess://`, etc.)

### Base64 URI parsing details
- `ss://base64(method:password)@host:port` or `ss://method:password@host:port`
- `vmess://base64_json` where JSON contains `v`, `ps`, `id`/`uid`, `add`/`hnb`, `port`/`pnt`, `aid`, `net`, `path`, `host`, `sni`
- `trojan://password@host:port?sni=xxx`
- `socks5://host:port`

### Base64 decoding
Use the `base64` crate (already in Cargo.toml). Handle:
- Standard base64 (`+` and `/`)
- URL-safe base64 (`-` and `_`)
- Missing padding (add `=` as needed)

### Test fixtures
1. `base64_sample.txt` — individually encoded lines (one URI per line, each base64-encoded)
2. `base64_blob.txt` — entire subscription as one base64 blob

## Global Constraints
- Edition 2024, workspace dependency pattern (`workspace = true`)
- Logging: `tracing` crate, never `log`
- Error handling: malformed entries skipped with warning, never panic
- Lint: `cargo clippy -- -D warnings`
- Commit format: `type(scope): description`

## Implementation Steps
1. Replace stub `base64_uri.rs` with full implementation
2. Create test fixtures
3. Run `cargo test -p proxy-sub --lib parser::base64_uri`
4. Run `cargo clippy -p proxy-sub -- -D warnings`
5. Commit: `feat(sub): add Base64 URI parser`
