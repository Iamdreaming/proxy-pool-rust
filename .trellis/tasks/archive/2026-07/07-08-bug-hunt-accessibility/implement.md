# Implement: high-risk batch

Validation after every step: `cargo test -p <crate>`; final `cargo test` + `cargo clippy --workspace --all-targets -- -D warnings`.

## Order

1. [x] B7 percent_decode UTF-8 (byte-wise hex parse; panic-safe) + tests
2. [x] B6 vmess numeric port + test
3. [x] B5 clash plugin-opts/port value types + test
4. [x] A1 Proxy proxy_connect_url() for Https + validator uses it + test
5. [x] B3 ProxyStore::add carry_forward_history + tests
6. [x] A2 quorum admission (pool) / strict (xray); drop httpbin default + tests
7. [x] A3 get_top_candidates + FREE_POOL_CANDIDATE_LIMIT=8 + tests
8. [x] B2 stale xray cleanup removes pool entry + routing rule
9. [x] B1 RoutingService + adrules install/rollback/remove + tests
10. [x] B4 copy_bidirectional in gateway
11. [x] full cargo test + clippy green; trellis-check verified (found+fixed B7 panic)
    — next: commit; push; watch CI; read-only dev checks

## Rollback points

Each numbered item is an independent commit-sized unit; revert that file group if a step
regresses tests. B1 and A2/A3 are the riskiest — validate their unit tests before moving on.
