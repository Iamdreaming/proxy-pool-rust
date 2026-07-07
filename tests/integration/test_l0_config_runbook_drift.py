"""Local drift checks for dev update config and no-SSH runbook docs."""

from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
COMPOSE_FILE = REPO_ROOT / "deploy" / "docker-compose.yml"
RUNBOOK_FILE = REPO_ROOT / "docs" / "dev-validation.md"
README_FILE = REPO_ROOT / "README.md"
STATUS_SOURCE = REPO_ROOT / "crates" / "proxy-core" / "src" / "status.rs"

APP_UPDATE_ENV = {
    "PROXY_POOL_UPDATE_ENABLED": "true",
    "PROXY_POOL_UPDATE_DOCKER_SOCKET": "/var/run/docker.sock",
    "PROXY_POOL_UPDATE_CONTAINER": "proxy-pool",
    "PROXY_POOL_UPDATE_IMAGE": "ghcr.io/iamdreaming/proxy-pool-rust:latest",
    "PROXY_POOL_UPDATE_WATCHTOWER_URL": "http://watchtower-proxy-pool:8080/v1/update",
}

RELEASE_FIELDS = (
    "release.git_hash",
    "release.configured_image",
    "release.update_enabled",
    "release.update_container",
    "release.image_repo",
    "release.image_tag",
    "release.watchtower_url",
)


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def squish(text: str) -> str:
    return " ".join(text.split())


def test_compose_declares_canonical_update_env_wiring():
    compose = read_text(COMPOSE_FILE)

    for key, value in APP_UPDATE_ENV.items():
        assert f"- {key}={value}" in compose

    assert "- PROXY_POOL_UPDATE_TOKEN=${PROXY_POOL_UPDATE_TOKEN:-proxy-pool-update}" in compose
    assert (
        "- WATCHTOWER_HTTP_API_TOKEN=${PROXY_POOL_UPDATE_TOKEN:-proxy-pool-update}"
        in compose
    )
    assert "image: containrrr/watchtower" in compose
    assert "command: --http-api-update --cleanup --label-enable" in compose


def test_runbook_documents_compose_update_env_and_token_pairing():
    runbook = read_text(RUNBOOK_FILE)

    for key, value in APP_UPDATE_ENV.items():
        assert f"`{key}={value}`" in runbook

    assert "`PROXY_POOL_UPDATE_TOKEN` in the app container matches" in runbook
    assert "`WATCHTOWER_HTTP_API_TOKEN` in the Watchtower container" in runbook
    assert "/var/run/docker.sock" in runbook
    assert "not a license for integration tests or agents to control the host directly" in squish(
        runbook
    )


def test_runbook_release_fields_match_status_contract():
    runbook = read_text(RUNBOOK_FILE)
    status_source = read_text(STATUS_SOURCE)

    for field in RELEASE_FIELDS:
        assert f"`{field}`" in runbook

    for field_name in (
        "git_hash",
        "configured_image",
        "update_enabled",
        "update_container",
        "image_repo",
        "image_tag",
        "watchtower_url",
    ):
        assert f"pub {field_name}:" in status_source


def test_operator_docs_do_not_use_obsolete_update_image_field():
    for path in (RUNBOOK_FILE, README_FILE):
        assert "release.update_image" not in read_text(path)


def test_runbook_preserves_no_ssh_and_no_routine_update_boundary():
    runbook = read_text(RUNBOOK_FILE)
    normalized = squish(runbook)

    assert "Do not SSH to the dev address for this workflow." in runbook
    assert "Direct host Docker CLI or Docker API access from the test runner." in runbook
    assert "Calling `update_service` as a routine status check." in runbook
    assert "This is not part of the default read-only validation checklist." in normalized
    assert "Treat that as a mutating deployment action, not as validation." in normalized
    assert "Do not SSH to the host just to print environment variables." in normalized


def test_watchtower_printenv_is_documented_as_non_recommended_check():
    runbook = read_text(RUNBOOK_FILE)
    normalized = squish(runbook)

    assert "containrrr/watchtower" in runbook
    assert "printenv" in runbook
    assert "docker compose exec watchtower-proxy-pool printenv" in runbook
    assert "is not a recommended token check" in normalized
    assert "Prefer the compose file, app-container update env" in normalized
