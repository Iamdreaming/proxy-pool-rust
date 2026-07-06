"""Layer 1: Service health and version verification."""

import pytest
import socket
import requests

from config import HOST, API_PORT, GW_PORT, MCP_PORT, API_BASE


class TestServiceHealth:
    """Verify the deployed service is alive and running the correct version."""

    def test_api_status_200(self, server_ready):
        """API /api/status returns 200 with expected structure."""
        data = server_ready
        assert "version" in data
        assert "git_hash" in data
        assert "uptime_sec" in data
        assert "pool" in data
        assert "redis" in data
        assert "warp" in data
        assert "xray" in data

    def test_api_healthz(self, api_client):
        """/api/healthz returns process liveness without dependency checks."""
        resp = api_client.get(f"{API_BASE}/api/healthz")
        assert resp.status_code == 200
        assert resp.json() == {"status": "ok"}

    def test_api_readyz(self, api_client):
        """/api/readyz returns structured dependency readiness."""
        resp = api_client.get(f"{API_BASE}/api/readyz")
        assert resp.status_code in (200, 503)
        data = resp.json()
        assert data["status"] in ("ok", "error")
        if resp.status_code == 503:
            assert data.get("message")

    def test_git_hash_matches(self, server_ready, expected_git_hash):
        """Deployed git_hash matches expected commit."""
        if not expected_git_hash:
            pytest.skip("No expected git hash provided")
        actual = server_ready["git_hash"]
        assert actual.startswith(expected_git_hash), (
            f"git_hash mismatch: expected {expected_git_hash}, got {actual}"
        )

    def test_pool_structure(self, server_ready):
        """Pool status has all protocol counts."""
        pool = server_ready["pool"]
        assert "http" in pool
        assert "https" in pool
        assert "socks5" in pool
        assert "total" in pool

    def test_gateway_port_open(self):
        """Gateway port is accepting connections."""
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(5)
        try:
            sock.connect((HOST, GW_PORT))
        except (ConnectionRefusedError, socket.timeout) as e:
            pytest.fail(f"Gateway port {GW_PORT} not reachable: {e}")
        finally:
            sock.close()

    def test_mcp_port_open(self):
        """MCP HTTP port is accepting connections."""
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(5)
        try:
            sock.connect((HOST, MCP_PORT))
        except (ConnectionRefusedError, socket.timeout) as e:
            pytest.fail(f"MCP port {MCP_PORT} not reachable: {e}")
        finally:
            sock.close()

    def test_api_port_open(self):
        """API port is accepting connections."""
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(5)
        try:
            sock.connect((HOST, API_PORT))
        except (ConnectionRefusedError, socket.timeout) as e:
            pytest.fail(f"API port {API_PORT} not reachable: {e}")
        finally:
            sock.close()
