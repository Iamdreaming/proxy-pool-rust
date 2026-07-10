# Design: MCP ops hardening

## R1 — non-blocking update_service

Current `update_service(&self)` runs inspect → pull → Watchtower inline and only
returns after the pull. Refactor:

- Add `UpdateStatusKind::InProgress` + `UpdateStatusSnapshot::in_progress(config)`.
- Extract the heavy body into an associated fn:
  ```rust
  async fn run_update(update_status: Arc<RwLock<UpdateStatusSnapshot>>,
                      config: UpdateServiceConfig)
  ```
  It performs the existing inspect/pull/inspect/compare/Watchtower steps and
  writes the terminal snapshot into `update_status`. (Currently these steps call
  `self.record_update_status`; change them to write the passed Arc directly.)
- `update_service(&self)` keeps the synchronous guards (disabled → record+return;
  missing token → record+return). On the happy path it:
  1. records `in_progress`,
  2. `tokio::spawn`s `run_update` wrapped in `tokio::time::timeout(300s, …)`; on
     elapse writes a `failed("update timed out after 300s")` snapshot,
  3. returns `{"status":"update_started","message":"...poll update_status..."}`.
- Because MCP is a separate process now, the spawned task survives the proxy-pool
  restart and records the terminal status; the caller polls `update_status`.

State machine: `in_progress` → {`updated` | `already_current` | `failed` |
`disabled`}. `update_status` already just reads the snapshot — no change beyond
the new variant.

## R2 — container_logs tool

- Param struct:
  ```rust
  struct ContainerLogsParam { container: Option<String>, tail: Option<u32> }
  ```
- Tool `container_logs`:
  - `container = params.container | config.container_name | "proxy-pool"`.
  - `tail = params.tail.unwrap_or(200).clamp(1, 1000)`.
  - path = `/containers/{container}/logs?stdout=1&stderr=1&timestamps=1&tail={tail}`.
  - returns `{"container":..., "tail":..., "logs": "<text>"}` or
    `{"status":"error","message":...}`.
- Docker socket read: the logs endpoint returns a stream that Docker terminates by
  closing the connection (or chunked). Add `docker_api_get_raw(socket, path)` that
  sends the GET and reads the **body until EOF** (Docker closes after the tail),
  splitting headers then returning raw body bytes. Do NOT reuse the JSON parser.
- Demux: non-TTY logs are multiplexed as repeated frames:
  `[stream(1) | 0 | 0 | 0 | size(4, big-endian)] + payload[size]`. Add a pure
  helper:
  ```rust
  fn demux_docker_log_stream(body: &[u8]) -> String
  ```
  Walk frames, append each payload (utf8-lossy). If the buffer doesn't look
  framed (no valid header / TTY container emits raw text), fall back to
  utf8-lossy of the whole body. Merge stdout+stderr in arrival order.
- `#[cfg(not(unix))]`: return a structured "not supported on this platform" error
  (mirrors existing docker helpers' cfg split).

## Files touched

- `crates/proxy-mcp/src/lib.rs`:
  - enum `UpdateStatusKind` + `in_progress` constructor.
  - refactor `update_service` → guards + spawn; new `run_update` assoc fn.
  - new `container_logs` tool + `ContainerLogsParam`.
  - `docker_api_get_raw` + `demux_docker_log_stream` (+ `#[cfg(not(unix))]` stubs).
- Tests: state-transition/shape for update snapshots; `demux_docker_log_stream`
  over a synthetic 2-frame buffer; tail clamp.

## Risks

- Detached task + timeout: ensure the 300s timeout wrapper always writes a
  terminal snapshot so status never sticks at `in_progress`.
- Log volume: `tail` clamp + 64 MiB read cap (reuse) bound memory.
- Demux robustness: fall back to raw text if framing is absent, so a TTY-mode or
  unexpected body still yields readable output.
