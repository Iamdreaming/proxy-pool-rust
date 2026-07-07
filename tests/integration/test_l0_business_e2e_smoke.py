"""Local tests for the business availability smoke runner."""

import json

import business_e2e_smoke as smoke


def test_business_target_expected_statuses():
    openai = smoke.BusinessTarget("openai", "https://api.openai.com/v1/models", (401,))
    github = smoke.BusinessTarget("github", "https://github.com/")

    assert openai.accepts_status(401)
    assert not openai.accepts_status(200)
    assert github.accepts_status(200)
    assert github.accepts_status(302)
    assert not github.accepts_status(404)


def test_hash_matches_accepts_prefix_or_empty_expected():
    assert smoke.hash_matches("abcdef1", "abc")
    assert smoke.hash_matches("abcdef1", "")
    assert not smoke.hash_matches("abcdef1", "123")
    assert not smoke.hash_matches(None, "abc")


def test_matrix_target_serialization_preserves_expected_statuses():
    targets = {target.name: target.to_matrix_target() for target in smoke.DEFAULT_TARGETS}
    roles = {
        target.name: "baseline" if target.baseline else "business"
        for target in smoke.DEFAULT_TARGETS
    }

    assert targets["cloudflare_trace"] == "https://www.cloudflare.com/cdn-cgi/trace"
    assert roles["cloudflare_trace"] == "baseline"
    assert roles["openai_api"] == "business"
    assert targets["openai_api"] == {
        "url": "https://api.openai.com/v1/models",
        "expected_statuses": [401],
    }
    assert targets["reddit"] == {
        "url": "https://www.reddit.com/",
        "expected_statuses": [200, 403, 429],
    }


def test_extract_candidates_uses_proxy_shape_and_fallback_protocol():
    payload = {
        "proxies": [
            {
                "proxy": {"host": "1.2.3.4", "port": 8080, "protocol": "http"},
                "score": {"score": 0.8},
            },
            {
                "proxy": {"host": "5.6.7.8", "port": 1080},
                "score": {"score": 0.7},
            },
            {"proxy": {"host": "missing-port"}},
            {"not_proxy": True},
        ]
    }

    assert smoke.extract_candidates(payload, "socks5") == [
        {
            "host": "1.2.3.4",
            "port": 8080,
            "protocol": "http",
            "score": {"score": 0.8},
        },
        {
            "host": "5.6.7.8",
            "port": 1080,
            "protocol": "socks5",
            "score": {"score": 0.7},
        },
    ]


def test_summary_requires_gateway_and_candidate_signal():
    target = {
        "host": "example",
        "api_base": "http://example:8000",
        "gateway_proxy": "http://example:9080",
        "protocols": ("http",),
    }
    good_gateway = smoke.CheckResult(
        "gateway:github",
        True,
        "ok",
        {"target_role": "business"},
    )
    bad_gateway = smoke.CheckResult("gateway:openai", False, "fail")
    good_candidate = smoke.CheckResult("candidate:http:1.2.3.4:8080", True, "ok")
    bad_candidate = smoke.CheckResult("candidate:http:1.2.3.4:8080", False, "fail")

    assert smoke.build_summary(target, [good_gateway, good_candidate])["ok"]
    assert smoke.build_summary(target, [good_gateway, bad_candidate])["ok"] is False
    assert smoke.build_summary(target, [bad_gateway, good_candidate])["ok"] is False


def test_candidate_source_failure_does_not_hide_working_candidate():
    target = {
        "host": "example",
        "api_base": "http://example:8000",
        "gateway_proxy": "http://example:9080",
        "protocols": ("http", "socks5"),
    }
    good_gateway = smoke.CheckResult(
        "gateway:github",
        True,
        "ok",
        {"target_role": "business"},
    )
    no_socks5 = smoke.CheckResult(
        "candidates:socks5",
        False,
        "no scored proxy candidates returned",
    )
    good_http_candidate = smoke.CheckResult("candidate:http:1.2.3.4:8080", True, "ok")

    assert smoke.build_summary(target, [good_gateway, no_socks5, good_http_candidate])["ok"]


def test_candidate_source_failure_fails_without_working_candidate():
    target = {
        "host": "example",
        "api_base": "http://example:8000",
        "gateway_proxy": "http://example:9080",
        "protocols": ("http",),
    }
    good_gateway = smoke.CheckResult(
        "gateway:github",
        True,
        "ok",
        {"target_role": "business"},
    )
    no_http = smoke.CheckResult(
        "candidates:http",
        False,
        "no scored proxy candidates returned",
    )

    assert smoke.build_summary(target, [good_gateway, no_http])["ok"] is False


def test_runtime_version_failure_blocks_business_summary():
    target = {
        "host": "example",
        "api_base": "http://example:8000",
        "gateway_proxy": "http://example:9080",
        "protocols": ("http",),
    }
    runtime_mismatch = smoke.CheckResult(
        "runtime_version",
        False,
        "runtime hash mismatch",
    )
    good_gateway = smoke.CheckResult(
        "gateway:github",
        True,
        "ok",
        {"target_role": "business"},
    )
    good_candidate = smoke.CheckResult("candidate:http:1.2.3.4:8080", True, "ok")

    assert (
        smoke.build_summary(target, [runtime_mismatch, good_gateway, good_candidate])["ok"]
        is False
    )


def test_runtime_version_skip_does_not_block_business_summary():
    target = {
        "host": "example",
        "api_base": "http://example:8000",
        "gateway_proxy": "http://example:9080",
        "protocols": ("http",),
    }
    skipped_runtime = smoke.CheckResult(
        "runtime_version",
        True,
        "skipped",
        skipped=True,
    )
    good_gateway = smoke.CheckResult(
        "gateway:github",
        True,
        "ok",
        {"target_role": "business"},
    )
    good_candidate = smoke.CheckResult("candidate:http:1.2.3.4:8080", True, "ok")

    assert smoke.build_summary(target, [skipped_runtime, good_gateway, good_candidate])["ok"]


def test_main_stops_after_runtime_precheck_failure(monkeypatch, capsys):
    class FakeSession:
        def close(self):
            pass

    def fail_if_called(*_args, **_kwargs):
        raise AssertionError("business checks should not run after stale runtime")

    monkeypatch.setattr(smoke, "make_direct_session", lambda: FakeSession())
    monkeypatch.setattr(
        smoke,
        "check_runtime_version",
        lambda *_args, **_kwargs: smoke.CheckResult(
            "runtime_version",
            False,
            "runtime hash mismatch",
        ),
    )
    monkeypatch.setattr(smoke, "run_gateway_checks", fail_if_called)
    monkeypatch.setattr(smoke, "run_candidate_checks", fail_if_called)

    exit_code = smoke.main(["--json"])

    assert exit_code == 1
    payload = json.loads(capsys.readouterr().out)
    assert payload["ok"] is False
    assert [result["name"] for result in payload["results"]] == ["runtime_version"]


def test_gateway_baseline_success_does_not_count_as_business_signal():
    target = {
        "host": "example",
        "api_base": "http://example:8000",
        "gateway_proxy": "http://example:9080",
        "protocols": ("http",),
    }
    baseline_gateway = smoke.CheckResult(
        "gateway:cloudflare_trace",
        True,
        "ok",
        {"target_role": "baseline"},
    )
    good_candidate = smoke.CheckResult("candidate:http:1.2.3.4:8080", True, "ok")

    assert smoke.build_summary(target, [baseline_gateway, good_candidate])["ok"] is False


def test_skip_results_remove_component_requirement():
    target = {
        "host": "example",
        "api_base": "http://example:8000",
        "gateway_proxy": "http://example:9080",
        "protocols": ("http",),
    }
    skipped_gateway = smoke.CheckResult("gateway", True, "skipped", skipped=True)
    good_candidate = smoke.CheckResult("candidate:http:1.2.3.4:8080", True, "ok")

    assert smoke.build_summary(target, [skipped_gateway, good_candidate])["ok"]


def test_runner_uses_only_observational_api_endpoints():
    forbidden = {operation for operation in smoke.FORBIDDEN_OPERATIONS if operation.startswith("/")}
    used_paths = {path for _, path in smoke.USED_API_ENDPOINTS}

    assert used_paths.isdisjoint(forbidden)
    assert ("GET", "/api/proxies/scores") in smoke.USED_API_ENDPOINTS
    assert ("POST", "/api/proxy/check-matrix") in smoke.USED_API_ENDPOINTS


def test_protocol_parser_accepts_repeated_and_comma_separated_values():
    assert smoke.parse_protocols(None) == smoke.DEFAULT_PROTOCOLS
    assert smoke.parse_protocols(["http,socks5", "http"]) == ("http", "socks5")
