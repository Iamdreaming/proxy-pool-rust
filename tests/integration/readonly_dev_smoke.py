"""Read-only post-push smoke runner for the dev deployment.

This command only queries public GitHub Actions, HTTP, and MCP read-only
surfaces. It never calls mutating MCP tools or host-level deployment controls.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from dataclasses import asdict, dataclass, field
from typing import Any

import requests

from config import API_BASE, DEFAULT_TIMEOUT, EXPECTED_GIT_HASH, HOST, MCP_BASE
from helpers.mcp_client import McpClient


READ_ONLY_MCP_TOOLS = ("service_status", "update_status")
UPDATE_STATUS_VALUES = {
    "never_triggered",
    "disabled",
    "already_current",
    "updated",
    "failed",
}


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


def hash_matches(actual: str | None, expected: str | None) -> bool:
    """Return true when no expected hash is set or actual starts with it."""
    if not expected:
        return True
    return bool(actual) and actual.startswith(expected)


def all_checks_ok(results: list[CheckResult]) -> bool:
    return all(result.ok or result.skipped for result in results)


def result_from_exception(name: str, exc: Exception, hint: str) -> CheckResult:
    return CheckResult(
        name=name,
        ok=False,
        summary=f"{type(exc).__name__}: {exc}",
        triage_hint=hint,
    )


def make_http_session() -> requests.Session:
    session = requests.Session()
    session.trust_env = False
    session.proxies = {"http": None, "https": None}
    return session


def run_gh(args: list[str], timeout: int) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["gh", *args],
        capture_output=True,
        check=False,
        text=True,
        timeout=timeout,
    )


def check_github_actions(
    branch: str,
    workflow: str,
    wait_ci: bool,
    timeout: int,
) -> CheckResult:
    name = "github_actions"
    if shutil.which("gh") is None:
        return CheckResult(
            name=name,
            ok=False,
            summary="GitHub CLI not found",
            triage_hint="Install gh or rerun with --skip-ci for HTTP/MCP-only smoke.",
        )

    fields = "databaseId,status,conclusion,headSha,url,displayTitle,createdAt,workflowName"
    list_args = [
        "run",
        "list",
        "--workflow",
        workflow,
        "--branch",
        branch,
        "--limit",
        "1",
        "--json",
        fields,
    ]

    try:
        list_result = run_gh(list_args, timeout)
    except Exception as exc:
        return result_from_exception(
            name,
            exc,
            "Run `gh run list --workflow=docker-build.yml --branch main --limit 1`.",
        )

    if list_result.returncode != 0:
        return CheckResult(
            name=name,
            ok=False,
            summary=list_result.stderr.strip() or "gh run list failed",
            triage_hint="Check GitHub CLI authentication with `gh auth status`.",
        )

    try:
        runs = json.loads(list_result.stdout)
    except json.JSONDecodeError as exc:
        return result_from_exception(
            name,
            exc,
            "Run `gh run list --limit 1` manually and inspect the output.",
        )

    if not runs:
        return CheckResult(
            name=name,
            ok=False,
            summary=f"No {workflow} runs found on {branch}",
            triage_hint="Push to the branch or check the workflow name.",
        )

    run = runs[0]
    run_id = str(run.get("databaseId") or "")
    status = run.get("status")
    conclusion = run.get("conclusion")

    if status != "completed" and wait_ci and run_id:
        watch = run_gh(["run", "watch", run_id, "--exit-status"], timeout)
        if watch.returncode != 0:
            return CheckResult(
                name=name,
                ok=False,
                summary=f"workflow run {run_id} did not complete successfully",
                details={"run_id": run_id, "status": status, "url": run.get("url")},
                triage_hint=f"Run `gh run view {run_id} --log-failed`.",
            )
        return check_github_actions(branch, workflow, wait_ci=False, timeout=timeout)

    details = {
        "run_id": run_id,
        "status": status,
        "conclusion": conclusion,
        "head_sha": run.get("headSha"),
        "url": run.get("url"),
        "title": run.get("displayTitle"),
        "created_at": run.get("createdAt"),
    }

    if status != "completed":
        return CheckResult(
            name=name,
            ok=False,
            summary=f"workflow run {run_id} is {status}",
            details=details,
            triage_hint=f"Run `gh run watch {run_id} --exit-status` or rerun with --wait-ci.",
        )

    if conclusion != "success":
        return CheckResult(
            name=name,
            ok=False,
            summary=f"workflow run {run_id} concluded {conclusion}",
            details=details,
            triage_hint=f"Run `gh run view {run_id} --log-failed`.",
        )

    short_sha = (run.get("headSha") or "")[:7]
    return CheckResult(
        name=name,
        ok=True,
        summary=f"{workflow} run {run_id} succeeded on {short_sha}",
        details=details,
    )


def evaluate_status_payload(
    payload: dict[str, Any],
    expected_hash: str | None,
    source: str,
) -> CheckResult:
    git_hash = payload.get("git_hash")
    release = payload.get("release") if isinstance(payload.get("release"), dict) else {}
    release_hash = release.get("git_hash")
    configured_image = release.get("configured_image")

    missing = [
        field_name
        for field_name, value in (
            ("git_hash", git_hash),
            ("release.git_hash", release_hash),
            ("release.configured_image", configured_image),
        )
        if not value
    ]
    if missing:
        return CheckResult(
            name=source,
            ok=False,
            summary=f"missing status fields: {', '.join(missing)}",
            details={"missing": missing},
            triage_hint="Check release status contract smoke coverage.",
        )

    if expected_hash and (
        not hash_matches(git_hash, expected_hash)
        or not hash_matches(release_hash, expected_hash)
    ):
        return CheckResult(
            name=source,
            ok=False,
            summary=f"runtime hash mismatch: expected {expected_hash}, got {git_hash}",
            details={
                "expected_hash": expected_hash,
                "git_hash": git_hash,
                "release_git_hash": release_hash,
                "configured_image": configured_image,
            },
            triage_hint="Check CI success and read-only MCP update_status before choosing any update.",
        )

    return CheckResult(
        name=source,
        ok=True,
        summary=f"runtime git_hash={git_hash}, image={configured_image}",
        details={
            "git_hash": git_hash,
            "release_git_hash": release_hash,
            "configured_image": configured_image,
            "update_enabled": release.get("update_enabled"),
            "update_container": release.get("update_container"),
            "watchtower_url": release.get("watchtower_url"),
        },
    )


def check_http_status(
    session: requests.Session,
    api_base: str,
    expected_hash: str | None,
    timeout: float,
) -> CheckResult:
    try:
        response = session.get(f"{api_base}/api/status", timeout=timeout)
        if response.status_code != 200:
            return CheckResult(
                name="http_status",
                ok=False,
                summary=f"/api/status returned HTTP {response.status_code}",
                triage_hint="Check the public API endpoint; do not SSH as the default next step.",
            )
        payload = response.json()
    except Exception as exc:
        return result_from_exception(
            "http_status",
            exc,
            "Check PROXY_POOL_HOST / PROXY_POOL_API_PORT and public API reachability.",
        )

    return evaluate_status_payload(payload, expected_hash, "http_status")


def check_http_readyz(
    session: requests.Session,
    api_base: str,
    timeout: float,
) -> CheckResult:
    try:
        response = session.get(f"{api_base}/api/readyz", timeout=timeout)
        payload = response.json()
    except Exception as exc:
        return result_from_exception(
            "http_readyz",
            exc,
            "Check the public /api/readyz endpoint and dependency readiness.",
        )

    status = payload.get("status") if isinstance(payload, dict) else None
    details = {
        "http_status": response.status_code,
        "status": status,
        "message": payload.get("message") if isinstance(payload, dict) else None,
    }
    if response.status_code == 200 and status == "ok":
        return CheckResult(
            name="http_readyz",
            ok=True,
            summary="/api/readyz is ok",
            details=details,
        )

    return CheckResult(
        name="http_readyz",
        ok=False,
        summary=f"/api/readyz is not ready: HTTP {response.status_code}, status={status}",
        details=details,
        triage_hint="Inspect /api/readyz response; do not use host Docker as the default check.",
    )


def extract_mcp_text(result: dict[str, Any] | str) -> str:
    if isinstance(result, str):
        return result
    content = result.get("content", [])
    for item in content:
        if item.get("type") == "text":
            return item["text"]
    return json.dumps(result)


def parse_mcp_json(result: dict[str, Any] | str) -> dict[str, Any]:
    text = extract_mcp_text(result)
    return json.loads(text)


def check_mcp_service_status(
    client: McpClient,
    expected_hash: str | None,
) -> CheckResult:
    try:
        payload = parse_mcp_json(client.call_tool(READ_ONLY_MCP_TOOLS[0]))
    except Exception as exc:
        return result_from_exception(
            "mcp_service_status",
            exc,
            "Check MCP HTTP transport and service_status shape.",
        )
    return evaluate_status_payload(payload, expected_hash, "mcp_service_status")


def check_mcp_update_status(client: McpClient) -> CheckResult:
    try:
        payload = parse_mcp_json(client.call_tool(READ_ONLY_MCP_TOOLS[1]))
    except Exception as exc:
        return result_from_exception(
            "mcp_update_status",
            exc,
            "Check MCP HTTP transport and update_status shape.",
        )

    status = payload.get("status")
    details = {
        "status": status,
        "update_enabled": payload.get("update_enabled"),
        "container_name": payload.get("container_name"),
        "image": payload.get("image"),
        "watchtower_url": payload.get("watchtower_url"),
        "message": payload.get("message"),
    }
    if status not in UPDATE_STATUS_VALUES:
        return CheckResult(
            name="mcp_update_status",
            ok=False,
            summary=f"unknown update_status={status}",
            details=details,
            triage_hint="Check MCP update_status contract smoke coverage.",
        )
    if status == "failed":
        return CheckResult(
            name="mcp_update_status",
            ok=False,
            summary=f"latest update attempt failed: {payload.get('message') or 'no message'}",
            details=details,
            triage_hint="Inspect update_status details before intentionally choosing any update.",
        )

    return CheckResult(
        name="mcp_update_status",
        ok=True,
        summary=f"update_status={status}",
        details=details,
    )


def check_mcp_read_only(
    mcp_base: str,
    expected_hash: str | None,
    timeout: float,
) -> list[CheckResult]:
    client = McpClient(mcp_base, timeout=timeout)
    try:
        return [
            check_mcp_service_status(client, expected_hash),
            check_mcp_update_status(client),
        ]
    finally:
        client.close()


def format_human_report(target: dict[str, Any], results: list[CheckResult]) -> str:
    lines = [
        "Read-only dev smoke",
        f"Target: host={target['host']} api={target['api_base']} mcp={target['mcp_base']}",
        f"Expected git hash: {target['expected_hash'] or '(not set)'}",
        "",
    ]
    for result in results:
        prefix = "skip" if result.skipped else ("ok" if result.ok else "fail")
        lines.append(f"[{prefix}] {result.name}: {result.summary}")
        if not result.ok and result.triage_hint:
            lines.append(f"       next: {result.triage_hint}")

    if all_checks_ok(results):
        lines.append("")
        lines.append("All enabled read-only checks passed.")
    else:
        lines.append("")
        lines.append("One or more read-only checks failed.")

    return "\n".join(lines)


def build_summary(target: dict[str, Any], results: list[CheckResult]) -> dict[str, Any]:
    return {
        "ok": all_checks_ok(results),
        "target": target,
        "results": [result.to_dict() for result in results],
    }


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run read-only dev smoke checks through GitHub Actions, HTTP, and MCP.",
    )
    parser.add_argument("--branch", default="main", help="Git branch for GitHub Actions lookup.")
    parser.add_argument(
        "--workflow",
        default="docker-build.yml",
        help="GitHub Actions workflow file/name to inspect.",
    )
    parser.add_argument("--skip-ci", action="store_true", help="Skip GitHub Actions checks.")
    parser.add_argument(
        "--wait-ci",
        action="store_true",
        help="Wait for the selected GitHub Actions run to finish.",
    )
    parser.add_argument(
        "--ci-timeout",
        type=int,
        default=900,
        help="Timeout in seconds for GitHub CLI operations.",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=DEFAULT_TIMEOUT,
        help="Timeout in seconds for HTTP and MCP requests.",
    )
    parser.add_argument(
        "--expected-git-hash",
        default=EXPECTED_GIT_HASH,
        help="Expected runtime git hash; defaults to PROXY_POOL_GIT_HASH.",
    )
    parser.add_argument("--json", action="store_true", help="Emit JSON output.")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    target = {
        "host": HOST,
        "api_base": API_BASE,
        "mcp_base": MCP_BASE,
        "branch": args.branch,
        "workflow": args.workflow,
        "expected_hash": args.expected_git_hash,
    }

    results: list[CheckResult] = []
    if args.skip_ci:
        results.append(
            CheckResult(
                name="github_actions",
                ok=True,
                skipped=True,
                summary="skipped by --skip-ci",
            )
        )
    else:
        results.append(
            check_github_actions(args.branch, args.workflow, args.wait_ci, args.ci_timeout)
        )

    session = make_http_session()
    try:
        results.append(
            check_http_status(session, API_BASE, args.expected_git_hash, args.timeout)
        )
        results.append(check_http_readyz(session, API_BASE, args.timeout))
    finally:
        session.close()

    results.extend(check_mcp_read_only(MCP_BASE, args.expected_git_hash, args.timeout))

    summary = build_summary(target, results)
    if args.json:
        print(json.dumps(summary, indent=2, sort_keys=True))
    else:
        print(format_human_report(target, results))
    return 0 if summary["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
