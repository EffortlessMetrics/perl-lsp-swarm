"""Static contributor-topology authority derived from existing repository sources."""

from __future__ import annotations

import hashlib
import json
import re
import tomllib
from pathlib import Path

SCHEMA = 1
PRODUCT_IDENTITY_PATH = "policy/product-identity.toml"
SYNC_PROTOCOL_PATH = "docs/swarm/sync-protocol.md"
PROMOTION_PROTOCOL = (
    "docs/swarm/sync-protocol.md"
    "#mechanics-history-preserving-complete-tree-merge"
)
EXPECTED_DEVELOPMENT_BRANCH = "main"
EXPECTED_PUBLICATION_BRANCH = "master"
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
REPO_RE = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")


class ContributorTopologyError(ValueError):
    """A static authority or captured observation is invalid."""


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def canonical_digest(value: object) -> str:
    raw = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode()
    return hashlib.sha256(raw).hexdigest()


def require_string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ContributorTopologyError(f"{label} must be a non-empty string")
    return value


def require_repository(value: object, label: str) -> str:
    result = require_string(value, label)
    if not REPO_RE.fullmatch(result):
        raise ContributorTopologyError(f"{label} must be an owner/name slug")
    return result


def optional_sha(value: object, label: str) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str) or not SHA_RE.fullmatch(value):
        raise ContributorTopologyError(f"{label} must be null or a full lowercase SHA")
    return value


def repository_branch(protocol: str, repository: str) -> str:
    name = repository.split("/", 1)[1]
    pattern = re.compile(
        rf"^\|\s*`{re.escape(name)}/([^`]+)`\s*\|\s*[^|]+\|\s*$",
        re.MULTILINE,
    )
    matches = pattern.findall(protocol)
    if len(matches) != 1:
        raise ContributorTopologyError(
            f"expected one authority row for {repository}; found {len(matches)}"
        )
    branch = matches[0].strip()
    if not branch or branch.startswith("-") or any(c.isspace() for c in branch):
        raise ContributorTopologyError(f"invalid branch for {repository}: {branch!r}")
    return branch


def load_static_topology(root: Path) -> tuple[dict[str, str], dict[str, dict[str, str]]]:
    identity_path = root / PRODUCT_IDENTITY_PATH
    protocol_path = root / SYNC_PROTOCOL_PATH
    try:
        identity = tomllib.loads(identity_path.read_text(encoding="utf-8"))
        protocol = protocol_path.read_text(encoding="utf-8")
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ContributorTopologyError(f"cannot load topology authority: {error}") from error

    if identity.get("schema_version") != 1:
        raise ContributorTopologyError("product identity schema must be 1")
    product = identity.get("product")
    if not isinstance(product, dict):
        raise ContributorTopologyError("product identity [product] table is missing")
    development_repo = require_repository(
        product.get("development_repository"), "product.development_repository"
    )
    publication_repo = require_repository(
        product.get("public_repository"), "product.public_repository"
    )
    if development_repo == publication_repo:
        raise ContributorTopologyError("development and publication repositories must differ")

    development_branch = repository_branch(protocol, development_repo)
    publication_branch = repository_branch(protocol, publication_repo)
    if development_branch != EXPECTED_DEVELOPMENT_BRANCH:
        raise ContributorTopologyError("development branch must be 'main'")
    if publication_branch != EXPECTED_PUBLICATION_BRANCH:
        raise ContributorTopologyError("publication branch must be 'master'")

    development_name = development_repo.split("/", 1)[1]
    publication_name = publication_repo.split("/", 1)[1]
    patterns = (
        rf"`{re.escape(development_name)}` is the active development source of truth\.",
        rf"`{re.escape(publication_name)}` is the\s+release, history, and canonical package-lineage repo\.",
    )
    if any(re.search(pattern, protocol) is None for pattern in patterns):
        raise ContributorTopologyError("sync protocol is missing repository-role authority")
    for marker in (
        "#### Mechanics: history-preserving complete-tree merge",
        f"git merge -s ours --no-commit swarm/{development_branch}",
        f"git read-tree -u --reset swarm/{development_branch}",
    ):
        if marker not in protocol:
            raise ContributorTopologyError(f"sync protocol is missing {marker!r}")

    static = {
        "development_repository": development_repo,
        "development_default_branch": development_branch,
        "publication_repository": publication_repo,
        "publication_branch": publication_branch,
        "issue_repository": development_repo,
        "pull_request_repository": development_repo,
        "promotion_protocol": PROMOTION_PROTOCOL,
    }
    sources = {
        PRODUCT_IDENTITY_PATH: {
            "path": PRODUCT_IDENTITY_PATH,
            "sha256": sha256(identity_path),
        },
        SYNC_PROTOCOL_PATH: {
            "path": SYNC_PROTOCOL_PATH,
            "sha256": sha256(protocol_path),
        },
    }
    return static, sources
