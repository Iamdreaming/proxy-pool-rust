"""Business availability smoke runner.

This command checks whether the public gateway and stored proxy candidates can
reach important business targets. It is observational only: it never refreshes,
deletes, updates, or otherwise mutates the running service.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from dataclasses import asdict, dataclass, field
from typing import Any

import requests

from config import API_BASE, DEFAULT_TIMEOUT, EXPECTED_GIT_HASH, GW_ADDR, HOST


GATEWAY_PROXY_URL = f"http://{GW_ADDR}"
DEFAULT_PROTOCOLS = ("http", "socks5")
USED_API_ENDPOINTS = (
    ("GET", "/api/proxies/scores"),
    ("POST", "/api/proxy/check-matrix"),
)
FORBIDDEN_OPERATIONS = {
    "/api/proxies/refresh",
    "/api/fetchers/{id}/refresh",
    "/api/subscriptions/sources/{id}/refresh",
    "/api/proxy/{key}",
    "refresh_pool",
    "refresh_fetcher",
    "refresh_subscription_source",
    "cleanup_low_score_proxies",
    "remove_proxy",
    "update_service",
}


@dataclass(frozen=True)
class BusinessTarget:
    name: str
    url: str
    expected_statuses: tuple[int, ...] = ()
    baseline: bool = False

    def accepts_status(self, status_code: int) -> bool:
        if self.expected_statuses:
            return status_code in self.expected_statuses
        return 200 <= status_code < 400

    def to_matrix_target(self) -> str | dict[str, Any]:
        if not self.expected_statuses:
            return self.url
        return {
            "url": self.url,
            "expected_statuses": list(self.expected_statuses),
        }


DEFAULT_TARGETS = (
    BusinessTarget(
        "cloudflare_trace",
        "https://www.cloudflare.com/cdn-cgi/trace",
        baseline=True,
    ),
    BusinessTarget("github", "https://github.com/"),
    BusinessTarget("openai_api", "https://api.openai.com/v1/models", (401,)),
    BusinessTarget("reddit", "https://www.reddit.com/", (200, 403, 429)),
)


@dataclass
class CheckResult:
    name: str
    ok: bool
    summary: str
    details: dict[str, Any] = field(default_factory=dict)
    triage_hint: str = ""
    skipped: bool = False

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


def make_direct_session() -> requests.Session:
    session = requests.Session()
    session.trust_env = False
    session.proxies = {"http": None, "https": None}
    session.headers.update({"user-agent": "proxy-pool-business-smoke/1.0"})
    return session


def make_gateway_session(proxy_url: str) -> requests.Session:
    session = requests.Session()
    session.trust_env = False
    session.proxies = {"http": proxy_url, "https": proxy_url}
    session.headers.update({"user-agent": "proxy-pool-business-smoke/1.0"})
    return session


def result_from_exception(
    name: str,
    exc: Exception,
    hint: str,
    details: dict[str, Any] | None = None,
) -> CheckResult:
    return CheckResult(
        name=name,
        ok=False,
        summary=f"{type(exc).__name__}: {exc}",
        details=details or {},
        triage_hint=hint,
    )


def hash_matches(actual: str | None, expected: str | None) -> bool:
    if not expected:
        return True
    return bool(actual) and actual.startswith(expected)


def current_git_hash() -> str:
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--short", "HEAD"],
            capture_output=True,
            check=False,
            text=True,
            timeout=3,
        )
    except Exception:
        return ""
    if result.returncode != 0:
        return ""
    return result.stdout.strip()


def resolve_expected_git_hash(configured_hash: str | None) -> str:
    return (configured_hash or "").strip() or current_git_hash()


def check_gateway_target(
    session: requests.Session,
    target: BusinessTarget,
    timeout: float,
) -> CheckResult:
    started = time.monotonic()
    try:
        response = session.get(
            target.url,
            timeout=timeout,
            allow_redirects=False,
            stream=True,
        )
        response.close()
    except Exception as exc:
        return result_from_exception(
            f"gateway:{target.name}",
            exc,
            "Check public gateway reachability and route/fallback diagnostics.",
            {"url": target.url, "expected_statuses": list(target.expected_statuses)},
        )

    elapsed_ms = round((time.monotonic() - started) * 1000.0, 2)
    ok = target.accepts_status(response.status_code)
    expected = (
        list(target.expected_statuses)
        if target.expected_statuses
        else ["2xx", "3xx"]
    )
    return CheckResult(
        name=f"gateway:{target.name}",
        ok=ok,
        summary=(
            f"HTTP {response.status_code} in {elapsed_ms}ms"
            if ok
            else f"unexpected HTTP {response.status_code} in {elapsed_ms}ms"
        ),
        details={
            "url": target.url,
            "target_role": "baseline" if target.baseline else "business",
            "http_status": response.status_code,
            "elapsed_ms": elapsed_ms,
            "expected_statuses": expected,
        },
        triage_hint=(
            ""
            if ok
            else "Inspect route_test, gateway fallback metrics, and proxy candidate checks."
        ),
    )


def check_runtime_version(
    session: requests.Session,
    api_base: str,
    expected_hash: str,
    timeout: float,
) -> CheckResult:
    try:
        response = session.get(f"{api_base}/api/status", timeout=timeout)
        response.raise_for_status()
        payload = response.json()
    except Exception as exc:
        return result_from_exception(
            "runtime_version",
            exc,
            "Check public /api/status before interpreting business target failures.",
        )

    git_hash = payload.get("git_hash")
    release = payload.get("release") if isinstance(payload.get("release"), dict) else {}
    release_hash = release.get("git_hash")
    details = {
        "expected_hash": expected_hash,
        "git_hash": git_hash,
        "release_git_hash": release_hash,
        "configured_image": release.get("configured_image"),
    }
    if expected_hash and (
        not hash_matches(git_hash, expected_hash)
        or not hash_matches(release_hash, expected_hash)
    ):
        return CheckResult(
            name="runtime_version",
            ok=False,
            summary=f"runtime hash mismatch: expected {expected_hash}, got {git_hash}",
            details=details,
            triage_hint=(
                "Deploy the latest image through CI/update flow before treating "
                "business target failures as current-code evidence."
            ),
        )

    return CheckResult(
        name="runtime_version",
        ok=True,
        summary=f"runtime git_hash={git_hash or '(unknown)'}",
        details=details,
    )


def fetch_scored_candidates(
    session: requests.Session,
    api_base: str,
    protocol: str,
    limit: int,
    timeout: float,
) -> list[dict[str, Any]]:
    response = session.get(
        f"{api_base}/api/proxies/scores",
        params={"protocol": protocol, "limit": limit},
        timeout=timeout,
    )
    response.raise_for_status()
    payload = response.json()
    return extract_candidates(payload, protocol)


def extract_candidates(payload: dict[str, Any], fallback_protocol: str) -> list[dict[str, Any]]:
    candidates = []
    for item in payload.get("proxies", []):
        proxy = item.get("proxy") if isinstance(item, dict) else None
        if not isinstance(proxy, dict):
            continue
        host = proxy.get("host")
        port = proxy.get("port")
        protocol = proxy.get("protocol") or fallback_protocol
        if isinstance(host, str) and isinstance(port, int) and isinstance(protocol, str):
            candidates.append(
                {
                    "host": host,
                    "port": port,
                    "protocol": protocol,
                    "score": item.get("score"),
                }
            )
    return candidates


def check_candidate_matrix(
    session: requests.Session,
    api_base: str,
    candidate: dict[str, Any],
    targets: tuple[BusinessTarget, ...],
    timeout: float,
) -> CheckResult:
    body = {
        "host": candidate["host"],
        "port": candidate["port"],
        "protocol": candidate["protocol"],
        "targets": [target.to_matrix_target() for target in targets],
        "timeout_secs": bounded_timeout_secs(timeout),
    }
    name = f"candidate:{candidate['protocol']}:{candidate['host']}:{candidate['port']}"
    try:
        response = session.post(
            f"{api_base}/api/proxy/check-matrix",
            json=body,
            timeout=max(timeout + 10.0, 30.0),
        )
        response.raise_for_status()
        payload = response.json()
    except Exception as exc:
        return result_from_exception(
            name,
            exc,
            "Check /api/proxy/check-matrix availability and candidate proxy shape.",
            {"candidate": candidate},
        )

    alive_count = int(payload.get("alive_count") or 0)
    target_count = int(payload.get("target_count") or 0)
    business_urls = {target.url for target in targets if not target.baseline}
    checks = payload.get("checks", [])
    business_alive_count = sum(
        1
        for check in checks
        if isinstance(check, dict)
        and check.get("alive")
        and check.get("target_url") in business_urls
    )
    ok = business_alive_count > 0
    return CheckResult(
        name=name,
        ok=ok,
        summary=(
            f"{business_alive_count}/{len(business_urls)} business targets reachable "
            f"({alive_count}/{target_count} total)"
        ),
        details={
            "candidate": candidate,
            "target_count": target_count,
            "alive_count": alive_count,
            "business_target_count": len(business_urls),
            "business_alive_count": business_alive_count,
            "failed_count": payload.get("failed_count"),
            "checks": checks,
        },
        triage_hint=(
            ""
            if ok
            else "Candidate exists but did not reach any configured business target."
        ),
    )


def bounded_timeout_secs(timeout: float) -> int:
    return max(1, min(60, int(round(timeout))))


def run_gateway_checks(
    proxy_url: str,
    targets: tuple[BusinessTarget, ...],
    timeout: float,
) -> list[CheckResult]:
    session = make_gateway_session(proxy_url)
    try:
        return [check_gateway_target(session, target, timeout) for target in targets]
    finally:
        session.close()


def run_candidate_checks(
    api_base: str,
    protocols: tuple[str, ...],
    candidate_limit: int,
    targets: tuple[BusinessTarget, ...],
    timeout: float,
) -> list[CheckResult]:
    session = make_direct_session()
    results: list[CheckResult] = []
    try:
        for protocol in protocols:
            try:
                candidates = fetch_scored_candidates(
                    session,
                    api_base,
                    protocol,
                    candidate_limit,
                    timeout,
                )
            except Exception as exc:
                results.append(
                    result_from_exception(
                        f"candidates:{protocol}",
                        exc,
                        "Check /api/proxies/scores and Redis readiness.",
                    )
                )
                continue

            if not candidates:
                results.append(
                    CheckResult(
                        name=f"candidates:{protocol}",
                        ok=False,
                        summary="no scored proxy candidates returned",
                        triage_hint="Check pool size, fetcher status, and source quality.",
                    )
                )
                continue

            for candidate in candidates[:candidate_limit]:
                results.append(
                    check_candidate_matrix(session, api_base, candidate, targets, timeout)
                )
    finally:
        session.close()
    return results


def component_has_signal(results: list[CheckResult], prefix: str) -> bool:
    scoped = [result for result in results if result.name.startswith(prefix)]
    if not scoped:
        return True
    if prefix == "gateway:":
        return any(
            result.ok and result.details.get("target_role", "business") == "business"
            for result in scoped
        )
    return any(result.ok for result in scoped)


def candidates_have_business_signal(results: list[CheckResult]) -> bool:
    if any(result.name == "candidates" and result.skipped for result in results):
        return True
    candidate_results = [
        result for result in results if result.name.startswith("candidate:")
    ]
    if candidate_results:
        return any(result.ok for result in candidate_results)
    source_results = [
        result for result in results if result.name.startswith("candidates:")
    ]
    if source_results:
        return any(result.ok for result in source_results)
    return True


def build_summary(target: dict[str, Any], results: list[CheckResult]) -> dict[str, Any]:
    return {
        "ok": all_required_prechecks_ok(results)
        and component_has_signal(results, "gateway:")
        and candidates_have_business_signal(results),
        "target": target,
        "results": [result.to_dict() for result in results],
    }


def all_required_prechecks_ok(results: list[CheckResult]) -> bool:
    prechecks = [
        result
        for result in results
        if not result.name.startswith(("gateway:", "candidate:", "candidates:"))
        and result.name != "candidates"
        and result.name != "gateway"
    ]
    return all(result.ok or result.skipped for result in prechecks)


def format_human_report(target: dict[str, Any], results: list[CheckResult]) -> str:
    lines = [
        "Business availability smoke",
        f"Target: host={target['host']} api={target['api_base']} gateway={target['gateway_proxy']}",
        f"Protocols: {', '.join(target['protocols'])}",
        f"Expected git hash: {target['expected_hash'] or '(not checked)'}",
        "",
    ]
    for result in results:
        prefix = "skip" if result.skipped else ("ok" if result.ok else "fail")
        lines.append(f"[{prefix}] {result.name}: {result.summary}")
        if not result.ok and result.triage_hint:
            lines.append(f"       next: {result.triage_hint}")

    summary = build_summary(target, results)
    lines.append("")
    if summary["ok"]:
        lines.append("Business availability signal found.")
    else:
        lines.append("No sufficient business availability signal found.")
    return "\n".join(lines)


def parse_protocols(raw_protocols: list[str] | None) -> tuple[str, ...]:
    if not raw_protocols:
        return DEFAULT_PROTOCOLS
    protocols: list[str] = []
    for raw in raw_protocols:
        for part in raw.split(","):
            protocol = part.strip().lower()
            if protocol:
                protocols.append(protocol)
    deduped = tuple(dict.fromkeys(protocols))
    return deduped or DEFAULT_PROTOCOLS


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run no-mutation business availability checks through gateway and API.",
    )
    parser.add_argument("--api-base", default=API_BASE, help="REST API base URL.")
    parser.add_argument(
        "--gateway-proxy",
        default=GATEWAY_PROXY_URL,
        help="HTTP proxy URL for the public gateway.",
    )
    parser.add_argument(
        "--protocol",
        action="append",
        help="Candidate protocol to scan. Repeat or comma-separate. Defaults to http,socks5.",
    )
    parser.add_argument(
        "--candidate-limit",
        type=int,
        default=3,
        help="Max scored proxy candidates to check per protocol.",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=DEFAULT_TIMEOUT,
        help="Timeout in seconds for gateway/API requests and matrix target checks.",
    )
    parser.add_argument(
        "--expected-git-hash",
        default=EXPECTED_GIT_HASH,
        help="Expected runtime git hash; defaults to PROXY_POOL_GIT_HASH or local HEAD.",
    )
    parser.add_argument(
        "--skip-version-check",
        action="store_true",
        help="Skip /api/status git hash precheck.",
    )
    parser.add_argument("--skip-gateway", action="store_true", help="Skip gateway checks.")
    parser.add_argument("--skip-candidates", action="store_true", help="Skip proxy candidate checks.")
    parser.add_argument("--json", action="store_true", help="Emit JSON output.")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    protocols = parse_protocols(args.protocol)
    expected_hash = "" if args.skip_version_check else resolve_expected_git_hash(args.expected_git_hash)
    target = {
        "host": HOST,
        "api_base": args.api_base,
        "gateway_proxy": args.gateway_proxy,
        "protocols": protocols,
        "candidate_limit": args.candidate_limit,
        "expected_hash": expected_hash,
        "targets": [asdict(target) for target in DEFAULT_TARGETS],
    }

    results: list[CheckResult] = []
    if args.skip_version_check:
        results.append(
            CheckResult(
                name="runtime_version",
                ok=True,
                summary="skipped by --skip-version-check",
                skipped=True,
            )
        )
    else:
        session = make_direct_session()
        try:
            version_result = check_runtime_version(
                session,
                args.api_base,
                expected_hash,
                args.timeout,
            )
        finally:
            session.close()
        results.append(version_result)
        if not version_result.ok:
            summary = build_summary(target, results)
            if args.json:
                print(json.dumps(summary, indent=2, sort_keys=True))
            else:
                print(format_human_report(target, results))
            return 1

    if args.skip_gateway:
        results.append(
            CheckResult(
                name="gateway",
                ok=True,
                summary="skipped by --skip-gateway",
                skipped=True,
            )
        )
    else:
        results.extend(run_gateway_checks(args.gateway_proxy, DEFAULT_TARGETS, args.timeout))

    if args.skip_candidates:
        results.append(
            CheckResult(
                name="candidates",
                ok=True,
                summary="skipped by --skip-candidates",
                skipped=True,
            )
        )
    else:
        results.extend(
            run_candidate_checks(
                args.api_base,
                protocols,
                args.candidate_limit,
                DEFAULT_TARGETS,
                args.timeout,
            )
        )

    summary = build_summary(target, results)
    if args.json:
        print(json.dumps(summary, indent=2, sort_keys=True))
    else:
        print(format_human_report(target, results))
    return 0 if summary["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
