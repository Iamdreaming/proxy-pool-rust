"""Lightweight live smoke for public no-SSH release validation surfaces."""

from config import API_BASE, EXPECTED_GIT_HASH
from helpers.release_status import (
    assert_readyz_contract,
    assert_status_payload_contract,
    assert_update_status_contract,
    parse_mcp_json,
)


def test_public_http_status_release_contract(api_client, server_ready):
    resp = api_client.get(f"{API_BASE}/api/status")
    assert resp.status_code == 200
    assert_status_payload_contract(resp.json(), EXPECTED_GIT_HASH)


def test_public_http_readyz_contract(api_client, server_ready):
    resp = api_client.get(f"{API_BASE}/api/readyz")
    assert_readyz_contract(resp.json(), resp.status_code)


def test_public_mcp_service_status_release_contract(mcp_client):
    payload = parse_mcp_json(mcp_client.call_tool("service_status"))
    assert_status_payload_contract(payload, EXPECTED_GIT_HASH)


def test_public_mcp_update_status_read_only_contract(mcp_client):
    payload = parse_mcp_json(mcp_client.call_tool("update_status"))
    assert_update_status_contract(payload)
