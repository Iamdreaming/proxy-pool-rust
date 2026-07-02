# proxy-server Spec Index

Guidelines for the `proxy-server` crate — the main entry point that assembles
and launches all proxy-pool services in a single process.

| Guide | Description |
|-------|-------------|
| [Directory Structure](./directory-structure.md) | Single-file architecture, startup sequence, service composition |
| [Error Handling](./error-handling.md) | Startup failures, service crash propagation, graceful degradation |
| [Quality Guidelines](./quality-guidelines.md) | Wiring conventions, Arc sharing, conditional features, forbidden patterns |
| [Logging Guidelines](./logging-guidelines.md) | Startup logging, service lifecycle events, log level conventions |
