# Design: validator-observability-multitarget

## Core model

Add matrix models to `proxy-core::validator`:

- `ProxyCheckMatrixRequest`: host, port, protocol, optional targets, optional timeout.
- `ProxyCheckMatrixResult`: proxy identity, target count, alive count, failed count, checks.

The matrix function should normalize target URLs once, reject empty or invalid URLs, and default to Cloudflare trace plus httpbin IP when no targets are supplied. Each normalized target creates a temporary `Validator` and calls `check_one()`, so timings, observed exit metadata, anonymity, and error typing stay in one place.

## API flow

`proxy-api` adds `POST /api/proxy/check-matrix`. The handler deserializes the core request type and calls the core matrix function. On validation errors it returns HTTP 400 with `SimpleResponse`; otherwise it returns the matrix result directly.

## MCP flow

`proxy-mcp` adds `CheckProxyMatrixParam`, mirroring the REST request fields. The tool converts params into the core request, calls the same matrix function, and pretty-prints the same core result. Existing `check_proxy` remains unchanged for single-target compatibility.

## Error handling

Input validation errors are deterministic and happen before network calls:

- blank host
- port zero
- unsupported protocol
- empty target string
- invalid target URL
- too many targets if a small safety limit is needed

Per-target network failures are not request errors; they appear as failed `ProxyCheckResult` entries.

## Testing

Core tests cover request validation and serialization. API/MCP tests cover deserialization and contract shape. Full network success is not required for unit tests; the matrix result can be tested through invalid input and synthetic serialization, while existing `Validator::check_one()` tests continue to cover per-target diagnostics.
