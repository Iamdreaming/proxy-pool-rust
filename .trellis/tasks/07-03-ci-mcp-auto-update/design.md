# Design: CI/CD + MCP Self-Update Stabilization

## Scope

This task closes the deploy/update loop already started in the repository and stabilizes the first production-facing operational surface. It covers CI image build, Docker Compose runtime wiring, MCP-triggered service update, version/status reporting, and startup failure behavior around the deploy path.

## Architecture

GitHub Actions builds `ghcr.io/iamdreaming/proxy-pool-rust` with `latest` and short-SHA tags. `deploy/docker-compose.yml` runs that image and a labeled Watchtower sidecar. The application exposes status through REST and MCP. The MCP `update_service` tool pre-pulls the configured image through the Docker Engine Unix socket, inspects old/new image identity, then triggers Watchtower through its HTTP API.

The update tool uses environment-derived configuration:

- `PROXY_POOL_UPDATE_ENABLED`: explicit safety switch.
- `PROXY_POOL_UPDATE_DOCKER_SOCKET`: Docker socket path.
- `PROXY_POOL_UPDATE_CONTAINER`: container to inspect before update.
- `PROXY_POOL_UPDATE_IMAGE`: image reference to pull.
- `PROXY_POOL_UPDATE_WATCHTOWER_URL`: Watchtower update endpoint.
- `PROXY_POOL_UPDATE_TOKEN`: bearer token for Watchtower.

Defaults may be developer-friendly, but updates are disabled unless the switch is true. The compose file enables the switch for the managed deployment and wires the token consistently into both proxy-pool and Watchtower.

## Data Flow

1. Operator or LLM calls MCP `update_service`.
2. Tool loads environment config and exits early if disabled.
3. Tool inspects the current container through Docker API.
4. Tool pulls the configured image and inspects the pulled image.
5. Tool compares previous image ID against pulled image ID / digest.
6. If changed, tool calls Watchtower HTTP API with bearer token.
7. Caller verifies `/api/status.git_hash` after the container restarts.

## Error Handling

Docker socket errors, Docker HTTP errors, image inspect failures, and Watchtower failures return structured JSON with `status: "error"` and enough context to diagnose the failing stage. Disabled updates return `status: "disabled"`.

API bind/serve failures remain fatal because the core service is unavailable. Optional background tasks should log errors and remain non-fatal where possible, especially when a secondary Redis connection for subscriptions fails.

## Compatibility

No Redis schema changes are included. No API route shapes are removed. Existing MCP tool names remain stable. The `update_service` response is extended with configuration-aware status fields.

## Out Of Scope

- Full Redis storage model migration.
- Route dry-run API/MCP.
- Authentication for all management APIs.
- Automatic rollback to a previous digest.
- Remote production update execution from this local session.

## Rollback

Revert the MCP update configuration changes and deploy compose environment changes. Since no persistent data format changes are included, rollback is limited to restarting the old image or reverting the commit.
