# Implementation Plan: free-source-expansion-v2

## Checklist

1. [x] Load `trellis-before-dev` and relevant specs.
2. [x] Add a shared public-list fetcher module.
3. [x] Wire new fetchers in `build_fetchers`.
4. [x] Add `FetchersConfig` toggles and defaults.
5. [x] Update `config/settings.example.yaml`.
6. [x] Add parser and config/build tests.
7. [x] Run:
   - [x] `cargo fmt --all --check`
   - [x] `cargo test -p proxy-core fetcher`
   - [x] `cargo test -p proxy-core config`
   - [x] `cargo test -p proxy-core`
   - [x] `cargo clippy -p proxy-core -- -D warnings`
   - [x] `cargo check --workspace`

## Implementation Notes

- Keep fetcher ids stable, such as `proxifly:all`, `databay:http`,
  `iplocate:all`, `vpslab:socks5`, and `monosans:json`.
- Do not integrate VPN account registration automation in this task.
- Do not bypass existing validation; all new candidates must flow through the
  scheduler's normal admission path.
