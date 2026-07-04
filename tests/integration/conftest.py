"""Shared pytest fixtures for integration tests."""

import pytest
import requests
import httpx

from config import HOST, API_PORT, GW_PORT, MCP_PORT, EXPECTED_GIT_HASH, API_BASE, MCP_BASE
from helpers.mcp_client import McpClient


# ---------------------------------------------------------------------------
# Server readiness
# ---------------------------------------------------------------------------

@pytest.fixture(scope="session")
def server_ready():
    """Wait for the server to be reachable on API port."""
    import time
    no_proxy = {"http": None, "https": None}
    for attempt in range(30):
        try:
            resp = requests.get(f"{API_BASE}/api/status", timeout=5, proxies=no_proxy)
            if resp.status_code == 200:
                return resp.json()
        except requests.ConnectionError:
            pass
        time.sleep(2)
    pytest.skip("Server not reachable after 60s")


@pytest.fixture(scope="session")
def server_ip():
    """Get the server's real (direct) IP by requesting ifconfig.me without proxy."""
    no_proxy = {"http": None, "https": None}
    try:
        resp = requests.get("https://ifconfig.me/ip", timeout=10, proxies=no_proxy)
        return resp.text.strip()
    except Exception:
        # Fallback: try from the status endpoint's perspective
        pytest.skip("Cannot determine server real IP")


# ---------------------------------------------------------------------------
# API clients
# ---------------------------------------------------------------------------

@pytest.fixture(scope="session")
def api_client():
    """HTTP client for REST API (bypasses system proxy)."""
    s = requests.Session()
    s.proxies = {"http": None, "https": None}  # bypass system proxy
    s.trust_env = False  # ignore env proxy settings
    yield s
    s.close()


@pytest.fixture(scope="session")
def socks5_proxy_client():
    """httpx client configured to use the gateway as SOCKS5 proxy.

    Requires httpx[socks] dependency: pip install httpx[socks]
    """
    try:
        proxy = f"socks5://{HOST}:{GW_PORT}"
        client = httpx.Client(proxy=proxy, timeout=20)
        yield client
        client.close()
    except Exception:
        pytest.skip("httpx[socks] not installed or SOCKS5 proxy unavailable")


@pytest.fixture(scope="session")
def mcp_client():
    """MCP client for tool calls."""
    client = McpClient(MCP_BASE)
    client.initialize()
    yield client
    client.close()


# ---------------------------------------------------------------------------
# Data fixtures
# ---------------------------------------------------------------------------

@pytest.fixture(scope="session")
def expected_git_hash():
    """Expected git hash from env or auto-detected."""
    if EXPECTED_GIT_HASH:
        return EXPECTED_GIT_HASH
    # Auto-detect from current git HEAD
    import subprocess
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--short", "HEAD"],
            capture_output=True, text=True, timeout=5,
        )
        return result.stdout.strip()
    except Exception:
        return ""
