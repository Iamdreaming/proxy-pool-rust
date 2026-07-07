"""Local tests for the read-only dev smoke runner."""

from readonly_dev_smoke import (
    CheckResult,
    READ_ONLY_MCP_TOOLS,
    all_checks_ok,
    evaluate_status_payload,
    extract_mcp_text,
    hash_matches,
)


def test_hash_matches_allows_unset_expected_hash():
    assert hash_matches("abcdef1", "")
    assert hash_matches("abcdef1", None)


def test_hash_matches_accepts_prefix():
    assert hash_matches("abcdef123456", "abcdef1")
    assert not hash_matches("abcdef123456", "1234567")


def test_all_checks_ok_treats_skipped_as_disabled():
    results = [
        CheckResult(name="ci", ok=True, skipped=True, summary="skipped"),
        CheckResult(name="http", ok=True, summary="ok"),
    ]
    assert all_checks_ok(results)


def test_all_checks_ok_fails_when_enabled_check_fails():
    results = [
        CheckResult(name="ci", ok=True, skipped=True, summary="skipped"),
        CheckResult(name="http", ok=False, summary="failed"),
    ]
    assert not all_checks_ok(results)


def test_evaluate_status_payload_accepts_expected_hash():
    payload = {
        "git_hash": "abcdef1",
        "release": {
            "git_hash": "abcdef1",
            "configured_image": "ghcr.io/iamdreaming/proxy-pool-rust:latest",
            "update_enabled": True,
            "update_container": "proxy-pool",
            "watchtower_url": "http://watchtower-proxy-pool:8080/v1/update",
        },
    }
    result = evaluate_status_payload(payload, "abc", "http_status")
    assert result.ok
    assert result.details["git_hash"] == "abcdef1"


def test_evaluate_status_payload_fails_hash_mismatch():
    payload = {
        "git_hash": "abcdef1",
        "release": {
            "git_hash": "abcdef1",
            "configured_image": "ghcr.io/iamdreaming/proxy-pool-rust:latest",
        },
    }
    result = evaluate_status_payload(payload, "123", "http_status")
    assert not result.ok
    assert "mismatch" in result.summary


def test_evaluate_status_payload_fails_missing_release_fields():
    payload = {"git_hash": "abcdef1", "release": {"git_hash": "abcdef1"}}
    result = evaluate_status_payload(payload, "", "http_status")
    assert not result.ok
    assert result.details["missing"] == ["release.configured_image"]


def test_extract_mcp_text_reads_text_content():
    result = {"content": [{"type": "text", "text": "{\"status\":\"ok\"}"}]}
    assert extract_mcp_text(result) == "{\"status\":\"ok\"}"


def test_runner_only_declares_read_only_mcp_tools():
    assert READ_ONLY_MCP_TOOLS == ("service_status", "update_status")
