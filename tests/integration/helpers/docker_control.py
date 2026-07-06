"""Safe integration helpers for deployment validation.

Fault injection must not use direct SSH or host Docker API access from the test
runner. Destructive container controls are unavailable until the service exposes
an explicit safe MCP/API operation for that scenario.
"""


class FaultInjectionUnavailable(RuntimeError):
    """Raised when a test asks for unsafe or unsupported fault injection."""


_FAULT_INJECTION_MESSAGE = (
    "WARP container fault injection is unavailable without an explicit safe "
    "MCP/API control surface; direct SSH or host Docker access is forbidden."
)


async def stop_warp_container(host: str, mcp_port: int = 9000) -> None:
    """Reject WARP fault injection until a safe MCP/API operation exists."""
    raise FaultInjectionUnavailable(_FAULT_INJECTION_MESSAGE)


async def start_warp_container(host: str, mcp_port: int = 9000) -> None:
    """Reject WARP restart until a safe MCP/API operation exists."""
    raise FaultInjectionUnavailable(_FAULT_INJECTION_MESSAGE)


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
