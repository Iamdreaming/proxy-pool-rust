"""Layer 4: MCP tool functional tests."""

import pytest
import json
import httpx

from helpers.mcp_client import McpClient
from config import MCP_BASE


class TestMcpConnection:
    """Test MCP server connectivity and initialization."""

    def test_mcp_initialize(self):
        """MCP server responds to initialize."""
        client = McpClient(MCP_BASE)
        try:
            result = client.initialize()
            assert "protocolVersion" in result
            assert "capabilities" in result
            assert "tools" in result["capabilities"]
        finally:
            client.close()

    def test_mcp_list_tools(self, mcp_client):
        """MCP server exposes expected tools."""
        result = mcp_client.list_tools()
        tools = result.get("tools", [])
        tool_names = {t["name"] for t in tools}

        expected = {
            "get_proxy", "get_best_proxy", "list_proxies",
            "check_proxy", "service_status", "pool_status", "warp_status",
            "geoip_lookup", "remove_proxy", "refresh_pool",
            "fetcher_status", "refresh_fetcher", "route_test",
            "explain_proxy_scores", "cleanup_low_score_proxies",
            "proxy_stats", "update_service",
        }
        missing = expected - tool_names
        assert not missing, f"Missing MCP tools: {missing}"


class TestMcpPoolStatus:
    """Test pool_status tool."""

    def test_pool_status_structure(self, mcp_client):
        """pool_status returns pool with protocol counts."""
        result = mcp_client.call_tool("pool_status")
        # Result is a list of content items; extract text
        text = _extract_text(result)
        data = json.loads(text)
        assert "pool" in data
        pool = data["pool"]
        assert "http" in pool
        assert "https" in pool
        assert "socks5" in pool
        assert "total" in pool

    def test_pool_status_total_matches(self, mcp_client):
        """Total count should equal sum of protocol counts."""
        result = mcp_client.call_tool("pool_status")
        text = _extract_text(result)
        data = json.loads(text)
        pool = data["pool"]
        expected_total = pool["http"] + pool["https"] + pool["socks5"]
        assert pool["total"] == expected_total


class TestMcpServiceStatus:
    """Test service_status tool."""

    def test_service_status_structure(self, mcp_client):
        """service_status returns the shared operator status structure."""
        result = mcp_client.call_tool("service_status")
        text = _extract_text(result)
        data = json.loads(text)

        assert data["version"]
        assert data["git_hash"]
        assert isinstance(data["uptime_sec"], int)
        assert "pool" in data
        assert "redis" in data
        assert "warp" in data
        assert "xray" in data
        assert data["redis"]["status"] in ("ok", "error")
        assert isinstance(data["pool"]["total"], int)
        assert isinstance(data["warp"]["configured"], int)
        assert isinstance(data["warp"]["healthy"], int)
        assert isinstance(data["xray"]["active_nodes"], int)


class TestMcpGetProxy:
    """Test get_proxy and get_best_proxy tools."""

    def test_get_proxy_returns_proxy_or_none(self, mcp_client):
        """get_proxy returns a proxy object or 'No proxy' message."""
        result = mcp_client.call_tool("get_proxy", {"protocol": "http"})
        text = _extract_text(result)
        if "No proxy" in text:
            pytest.skip("No http proxies available")
        data = json.loads(text)
        assert "host" in data
        assert "port" in data

    def test_get_best_proxy_returns_proxy_or_none(self, mcp_client):
        """get_best_proxy returns a proxy object or 'No proxy' message."""
        result = mcp_client.call_tool("get_best_proxy", {"protocol": "http"})
        text = _extract_text(result)
        if "No proxy" in text:
            pytest.skip("No http proxies available")
        data = json.loads(text)
        assert "host" in data
        assert "port" in data


class TestMcpCheckProxy:
    """Test check_proxy tool."""

    def test_check_proxy_alive(self, mcp_client):
        """Check a known-working proxy returns alive=true."""
        # First get a proxy from the pool
        result = mcp_client.call_tool("get_proxy", {"protocol": "http"})
        text = _extract_text(result)
        if "No proxy" in text:
            pytest.skip("No http proxies available")
        proxy = json.loads(text)

        # Check it
        check_result = mcp_client.call_tool("check_proxy", {
            "host": proxy["host"],
            "port": proxy["port"],
            "protocol": "http",
        })
        check_text = _extract_text(check_result)
        check_data = json.loads(check_text)
        assert "alive" in check_data
        assert "host" in check_data
        assert "port" in check_data
        assert "protocol" in check_data
        if not check_data["alive"]:
            assert check_data.get("error_type")
        # alive may be true or false depending on proxy health at test time
        # We just verify the structure is correct


class TestMcpFetcherStatus:
    """Test fetcher_status tool."""

    def test_fetcher_status_structure(self, mcp_client):
        result = mcp_client.call_tool("fetcher_status")
        text = _extract_text(result)
        data = json.loads(text)
        assert "fetchers" in data
        assert isinstance(data["fetchers"], list)
        if data["fetchers"]:
            first = data["fetchers"][0]
            assert "id" in first
            assert "name" in first
            assert first["status"] in ("never_run", "success", "empty", "error")
            assert isinstance(first["fetched"], int)
            assert isinstance(first["parsed"], int)


class TestMcpRouteTest:
    """Test route_test tool."""

    def test_route_test_structure(self, mcp_client):
        result = mcp_client.call_tool("route_test", {"host": "github.com", "protocol": "http"})
        text = _extract_text(result)
        data = json.loads(text)
        assert data["status"] == "ok"
        decision = data["decision"]
        assert decision["host"] == "github.com"
        assert decision["protocol"] == "http"
        assert decision["selected"] in ("direct", "free_pool", "warp", "xray", "no_proxy")
        assert isinstance(decision["candidates"], list)
        assert isinstance(decision["unavailable"], list)


class TestMcpGeoIPLookup:
    """Test geoip_lookup tool."""

    def test_overseas_domain(self, mcp_client):
        """google.com should be identified as overseas."""
        result = mcp_client.call_tool("geoip_lookup", {"host": "google.com"})
        text = _extract_text(result)
        if "not configured" in text:
            pytest.skip("GeoIP not configured on server")
        data = json.loads(text)
        assert data.get("is_overseas") is True, f"google.com should be overseas, got: {data}"

    def test_domestic_domain(self, mcp_client):
        """baidu.com should be identified as domestic (CN)."""
        result = mcp_client.call_tool("geoip_lookup", {"host": "baidu.com"})
        text = _extract_text(result)
        if "not configured" in text:
            pytest.skip("GeoIP not configured on server")
        data = json.loads(text)
        assert data.get("is_overseas") is False, f"baidu.com should be domestic, got: {data}"
        assert data.get("country") == "CN"

    def test_ip_lookup(self, mcp_client):
        """Direct IP lookup should return country info."""
        result = mcp_client.call_tool("geoip_lookup", {"host": "1.1.1.1"})
        text = _extract_text(result)
        if "not configured" in text:
            pytest.skip("GeoIP not configured on server")
        data = json.loads(text)
        assert data.get("country") is not None


class TestMcpRefreshPool:
    """Test refresh_pool tool."""

    @pytest.mark.slow
    def test_refresh_returns_ok(self, mcp_client):
        """Refresh pool should return ok with counts. Marked slow (takes minutes)."""
        # Use a longer timeout for refresh
        old_timeout = mcp_client._client.timeout
        mcp_client._client.timeout = httpx.Timeout(300.0)
        try:
            result = mcp_client.call_tool("refresh_pool")
            text = _extract_text(result)
            data = json.loads(text)
            assert data["status"] in ("ok", "error")
            if data["status"] == "ok":
                assert isinstance(data["fetched"], int)
                assert isinstance(data["stored"], int)
        finally:
            mcp_client._client.timeout = old_timeout

    @pytest.mark.slow
    def test_refresh_produces_proxies_with_geoip(self, mcp_client):
        """After refresh, proxies should have GeoIP data. Marked slow."""
        old_timeout = mcp_client._client.timeout
        mcp_client._client.timeout = httpx.Timeout(300.0)
        try:
            result = mcp_client.call_tool("refresh_pool")
            text = _extract_text(result)
            data = json.loads(text)
            if data["status"] != "ok" or data.get("stored", 0) == 0:
                pytest.skip("Refresh did not store any proxies")

            # Check that stored proxies have country
            list_result = mcp_client.call_tool("list_proxies", {"limit": 10})
            list_text = _extract_text(list_result)
            list_data = json.loads(list_text)
            proxies = list_data.get("proxies", [])
            with_country = [p for p in proxies if p.get("country")]
            assert len(with_country) > 0, "No proxies have country after refresh — GeoIP enrichment broken"
        finally:
            mcp_client._client.timeout = old_timeout


class TestMcpWarpStatus:
    """Test warp_status tool."""

    def test_warp_status_structure(self, mcp_client):
        """warp_status returns warp info with healthy_count."""
        result = mcp_client.call_tool("warp_status")
        text = _extract_text(result)
        if "not configured" in text:
            pytest.skip("WARP not configured")
        data = json.loads(text)
        assert "warp" in data
        assert "healthy_count" in data["warp"]


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _extract_text(result: dict) -> str:
    """Extract text content from MCP tool call result."""
    content = result.get("content", [])
    for item in content:
        if item.get("type") == "text":
            return item["text"]
    # Fallback: result might be the text directly
    if isinstance(result, str):
        return result
    return json.dumps(result)
