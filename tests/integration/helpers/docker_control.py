"""Docker control helpers for fault injection tests.

These functions require SSH access to the server or direct Docker API access.
For integration tests running from outside the server, they use the MCP
update_service and docker APIs through the proxy-pool container's socket.
"""

import httpx


async def stop_warp_container(host: str, mcp_port: int = 9000) -> None:
    """Stop all WARP containers via Docker API to simulate WARP failure.

    Uses the proxy-pool container's Docker socket access.
    """
    # WARP instances run as separate containers managed by docker-compose
    # We stop them via the Docker Engine API exposed through the proxy-pool container
    pass  # TODO: implement when WARP containers are accessible


async def start_warp_container(host: str, mcp_port: int = 9000) -> None:
    """Restart WARP containers after fault injection."""
    pass  # TODO: implement when WARP containers are accessible


async def clear_proxy_pool(host: str, api_port: int = 8000) -> int:
    """Delete all proxies from the pool via REST API. Returns count deleted."""
    import requests
    base = f"http://{host}:{api_port}"
    deleted = 0
    for protocol in ["http", "https", "socks5"]:
        resp = requests.get(f"{base}/api/proxies", params={"protocol": protocol, "limit": 500}, timeout=15)
        if resp.status_code != 200:
            continue
        data = resp.json()
        for proxy in data.get("proxies", []):
            key = f"{proxy['protocol']}:{proxy['host']}:{proxy['port']}"
            del_resp = requests.delete(f"{base}/api/proxy/{key}", timeout=10)
            if del_resp.status_code == 200:
                deleted += 1
    return deleted
