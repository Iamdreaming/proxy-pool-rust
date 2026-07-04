"""Layer 2: REST API functional tests."""

import pytest
import requests

from config import API_BASE


class TestApiStatus:
    """Test /api/status endpoint."""

    def test_status_returns_version(self, api_client):
        resp = api_client.get(f"{API_BASE}/api/status")
        assert resp.status_code == 200
        data = resp.json()
        assert data["version"]  # non-empty string
        assert data["git_hash"]  # non-empty string


class TestApiProxies:
    """Test proxy CRUD endpoints."""

    def test_list_proxies_default(self, api_client):
        """List proxies returns valid structure with default params."""
        resp = api_client.get(f"{API_BASE}/api/proxies")
        assert resp.status_code == 200
        data = resp.json()
        assert "protocol" in data
        assert "count" in data
        assert "proxies" in data
        assert isinstance(data["proxies"], list)

    def test_list_proxies_with_protocol(self, api_client):
        """List proxies filters by protocol."""
        for proto in ["http", "https", "socks5"]:
            resp = api_client.get(f"{API_BASE}/api/proxies", params={"protocol": proto})
            assert resp.status_code == 200
            data = resp.json()
            assert data["protocol"] == proto

    def test_list_proxies_with_limit(self, api_client):
        """List proxies respects limit parameter."""
        resp = api_client.get(f"{API_BASE}/api/proxies", params={"limit": 2})
        assert resp.status_code == 200
        data = resp.json()
        assert data["count"] <= 2

    def test_proxy_has_geoip_fields(self, api_client):
        """Proxies should have country and is_overseas fields populated."""
        resp = api_client.get(f"{API_BASE}/api/proxies", params={"protocol": "http", "limit": 10})
        assert resp.status_code == 200
        proxies = resp.json()["proxies"]
        if not proxies:
            pytest.skip("No http proxies in pool")
        # At least some proxies should have country set (after GeoIP enrichment)
        with_country = [p for p in proxies if p.get("country")]
        assert len(with_country) > 0, "No proxies have country field set — GeoIP enrichment may be broken"

    def test_get_random_proxy(self, api_client):
        """Get a random proxy returns valid structure."""
        resp = api_client.get(f"{API_BASE}/api/proxy/random", params={"protocol": "http"})
        assert resp.status_code == 200
        data = resp.json()
        if data is not None:
            assert "host" in data
            assert "port" in data
            assert "protocol" in data

    def test_get_best_proxy(self, api_client):
        """Get best proxy returns valid structure."""
        resp = api_client.get(f"{API_BASE}/api/proxy/best", params={"protocol": "http"})
        assert resp.status_code == 200
        data = resp.json()
        if data is not None:
            assert "host" in data
            assert "port" in data


class TestApiWarp:
    """Test WARP status endpoint."""

    def test_warp_status(self, api_client):
        resp = api_client.get(f"{API_BASE}/api/warp")
        assert resp.status_code == 200
        data = resp.json()
        assert "instances" in data
        assert isinstance(data["instances"], list)


class TestApiXray:
    """Test Xray status endpoint."""

    def test_xray_status(self, api_client):
        resp = api_client.get(f"{API_BASE}/api/xray/status")
        assert resp.status_code == 200
        data = resp.json()
        assert "active_nodes" in data
        assert isinstance(data["active_nodes"], int)


class TestApiMetrics:
    """Test Prometheus metrics endpoint."""

    def test_metrics_format(self, api_client):
        resp = api_client.get(f"{API_BASE}/api/metrics")
        assert resp.status_code == 200
        text = resp.text
        assert "proxy_pool_size" in text


class TestApiRefresh:
    """Test pool refresh endpoint."""

    @pytest.mark.slow
    def test_refresh_returns_ok(self, api_client):
        """Trigger a refresh and verify response structure.

        Marked slow because refresh involves fetching and validating proxies,
        which can take several minutes.
        """
        resp = api_client.post(f"{API_BASE}/api/proxies/refresh", timeout=300)
        assert resp.status_code == 200
        data = resp.json()
        assert data["status"] in ("ok", "error")
        if data["status"] == "ok":
            assert isinstance(data["fetched"], int)
            assert isinstance(data["validated"], int)
            assert isinstance(data["stored"], int)
            assert isinstance(data["errors"], int)


class TestApiDeleteProxy:
    """Test proxy deletion."""

    def test_delete_nonexistent_proxy(self, api_client):
        """Delete a non-existent proxy returns 404."""
        resp = api_client.delete(f"{API_BASE}/api/proxy/http:0.0.0.0:1")
        assert resp.status_code == 404
