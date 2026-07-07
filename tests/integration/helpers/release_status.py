"""Shared assertions for public release-status smoke checks.

These helpers intentionally cover only read-only release validation fields.
They must not call HTTP, MCP, Docker, or SSH themselves.
"""

from __future__ import annotations

import json
from typing import Any


KNOWN_UPDATE_STATUSES = {
    "never_triggered",
    "disabled",
    "already_current",
    "updated",
    "failed",
}

READ_ONLY_RELEASE_MCP_TOOLS = ("service_status", "update_status")
QUALITY_REASON_VALUES = {
    "unknown",
    "validation_failed",
    "timeout",
    "bad_status",
    "body_read_failed",
    "invalid_proxy_url",
    "client_build_failed",
    "request_failed",
    "circuit_open",
    "other",
}


def extract_mcp_text(result: dict[str, Any] | str) -> str:
    """Extract the first text content item from an MCP tool response."""
    if isinstance(result, str):
        return result
    content = result.get("content", [])
    for item in content:
        if item.get("type") == "text":
            return item["text"]
    return json.dumps(result)


def parse_mcp_json(result: dict[str, Any] | str) -> dict[str, Any]:
    """Parse the JSON text returned by an MCP tool call."""
    return json.loads(extract_mcp_text(result))


def assert_status_payload_contract(
    data: dict[str, Any],
    expected_hash: str | None = None,
) -> None:
    """Assert the shared `/api/status` / MCP `service_status` shape."""
    assert data["version"]
    assert data["git_hash"]
    assert isinstance(data["uptime_sec"], int)

    if expected_hash:
        assert data["git_hash"].startswith(expected_hash)

    assert_release_contract(data, expected_hash)
    assert_pool_contract(data["pool"])
    assert_redis_contract(data["redis"])
    assert_quality_contract(data["quality"])
    assert_warp_contract(data["warp"])
    assert_xray_contract(data["xray"])


def assert_release_contract(
    status_payload: dict[str, Any],
    expected_hash: str | None = None,
) -> None:
    release = status_payload["release"]
    assert release["app_version"] == status_payload["version"]
    assert release["git_hash"] == status_payload["git_hash"]
    if expected_hash:
        assert release["git_hash"].startswith(expected_hash)
    assert isinstance(release["update_enabled"], bool)
    assert release["update_container"]
    assert release["configured_image"]
    assert release["image_repo"]
    assert release["image_tag"]
    assert release["watchtower_url"]


def assert_pool_contract(pool: dict[str, Any]) -> None:
    assert isinstance(pool["http"], int)
    assert isinstance(pool["https"], int)
    assert isinstance(pool["socks5"], int)
    assert isinstance(pool["total"], int)
    assert pool["total"] == pool["http"] + pool["https"] + pool["socks5"]


def assert_redis_contract(redis: dict[str, Any]) -> None:
    assert redis["status"] in ("ok", "error")


def assert_quality_contract(quality: dict[str, Any]) -> None:
    assert isinstance(quality["total"], int)
    buckets = quality["score_buckets"]
    for bucket in ("untested", "poor", "fair", "good", "excellent"):
        assert isinstance(buckets[bucket], int)
    assert isinstance(quality["recent_samples"], int)
    assert quality["recent_success_rate"] is None or isinstance(
        quality["recent_success_rate"],
        (int, float),
    )
    assert isinstance(quality["recent_failures"], int)
    assert isinstance(quality["stale_proxies"], int)
    assert isinstance(quality["stale_after_secs"], int)

    retention = quality["retention"]
    assert isinstance(retention["below_min_score"], int)
    assert isinstance(retention["hard_failure_evict"], int)

    assert isinstance(quality["top_failure_reasons"], list)
    for reason in quality["top_failure_reasons"]:
        assert reason["reason"] in QUALITY_REASON_VALUES
        assert isinstance(reason["count"], int)


def assert_warp_contract(warp: dict[str, Any]) -> None:
    assert isinstance(warp["configured"], int)
    assert isinstance(warp["healthy"], int)


def assert_xray_contract(xray: dict[str, Any]) -> None:
    assert isinstance(xray["enabled"], bool)
    assert isinstance(xray["active_nodes"], int)
    assert isinstance(xray["failed_nodes"], int)
    assert isinstance(xray["removed_nodes"], int)
    assert isinstance(xray["total_nodes"], int)


def assert_readyz_contract(payload: dict[str, Any], http_status: int) -> None:
    """Assert the public readiness endpoint shape.

    Dev may return 503 while dependencies are unhealthy, but the body should
    still remain structured enough for no-SSH triage.
    """
    assert http_status in (200, 503)
    assert payload["status"] in ("ok", "error")
    if "message" in payload:
        assert isinstance(payload["message"], str)


def assert_update_status_contract(payload: dict[str, Any]) -> None:
    """Assert MCP `update_status` without requiring a mutation first."""
    status = payload["status"]
    assert status in KNOWN_UPDATE_STATUSES
    if status == "never_triggered":
        return

    assert isinstance(payload["update_enabled"], bool)
    assert payload["container_name"]
    assert payload["image"]
    assert payload["image_repo"]
    assert payload["image_tag"]
    assert payload["watchtower_url"]
    if status == "failed":
        assert payload.get("message")
