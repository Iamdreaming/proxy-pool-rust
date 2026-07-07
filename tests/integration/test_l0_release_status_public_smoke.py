"""Local tests for the lightweight public release-status smoke contract."""

from helpers.release_status import (
    KNOWN_UPDATE_STATUSES,
    READ_ONLY_RELEASE_MCP_TOOLS,
    assert_readyz_contract,
    assert_status_payload_contract,
    assert_update_status_contract,
    extract_mcp_text,
    parse_mcp_json,
)


def _status_payload() -> dict:
    return {
        "version": "0.1.0",
        "git_hash": "abcdef1",
        "uptime_sec": 12,
        "release": {
            "app_version": "0.1.0",
            "git_hash": "abcdef1",
            "update_enabled": True,
            "update_container": "proxy-pool",
            "configured_image": "ghcr.io/iamdreaming/proxy-pool-rust:latest",
            "image_repo": "ghcr.io/iamdreaming/proxy-pool-rust",
            "image_tag": "latest",
            "watchtower_url": "http://watchtower-proxy-pool:8080/v1/update",
        },
        "pool": {"http": 2, "https": 1, "socks5": 3, "total": 6},
        "redis": {"status": "ok"},
        "quality": {
            "total": 6,
            "score_buckets": {
                "untested": 0,
                "poor": 2,
                "fair": 3,
                "good": 1,
                "excellent": 0,
            },
            "recent_samples": 10,
            "recent_success_rate": 0.7,
            "recent_failures": 3,
            "stale_proxies": 0,
            "stale_after_secs": 3600,
            "retention": {
                "below_min_score": 1,
                "hard_failure_evict": 0,
            },
            "top_failure_reasons": [
                {"reason": "validation_failed", "count": 3},
            ],
        },
        "warp": {"configured": 3, "healthy": 2},
        "xray": {
            "enabled": False,
            "active_nodes": 0,
            "failed_nodes": 0,
            "removed_nodes": 0,
            "total_nodes": 0,
        },
    }


def test_status_payload_contract_accepts_public_release_shape():
    assert_status_payload_contract(_status_payload(), expected_hash="abc")


def test_status_payload_contract_rejects_hash_mismatch():
    try:
        assert_status_payload_contract(_status_payload(), expected_hash="123")
    except AssertionError:
        return
    raise AssertionError("expected hash mismatch to fail")


def test_readyz_contract_accepts_ok_and_error_shapes():
    assert_readyz_contract({"status": "ok"}, 200)
    assert_readyz_contract({"status": "error", "message": "redis unavailable"}, 503)


def test_update_status_contract_allows_known_statuses():
    assert KNOWN_UPDATE_STATUSES == {
        "never_triggered",
        "disabled",
        "already_current",
        "updated",
        "failed",
    }
    assert_update_status_contract({"status": "never_triggered"})
    assert_update_status_contract({
        "status": "updated",
        "update_enabled": True,
        "container_name": "proxy-pool",
        "image": "ghcr.io/iamdreaming/proxy-pool-rust:latest",
        "image_repo": "ghcr.io/iamdreaming/proxy-pool-rust",
        "image_tag": "latest",
        "watchtower_url": "http://watchtower-proxy-pool:8080/v1/update",
    })


def test_release_status_smoke_only_declares_read_only_mcp_tools():
    assert READ_ONLY_RELEASE_MCP_TOOLS == ("service_status", "update_status")


def test_mcp_text_helpers_parse_tool_content():
    result = {"content": [{"type": "text", "text": "{\"status\":\"ok\"}"}]}
    assert extract_mcp_text(result) == "{\"status\":\"ok\"}"
    assert parse_mcp_json(result) == {"status": "ok"}
