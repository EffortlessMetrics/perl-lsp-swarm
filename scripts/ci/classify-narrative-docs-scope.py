#!/usr/bin/env python3
"""Fail-closed classifier for the audited narrative-document CI no-op scope.

The workflow downloads this program and its policy from the pull request's exact
base commit.  The program therefore treats every malformed, stale, incomplete,
or unaudited subject as a typed full-proof decision rather than raising an
exception that could be mistaken for a skip.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import sys
import tomllib
from typing import Any


SCHEMA_VERSION = 1
CLASSIFIER_IDENTITY = "classify-narrative-docs-scope.py:v1"
POLICY_IDENTITY = "ci-narrative-docs-scope:v1"
AUDITED_PATHS = ["docs/project/status/release.md"]
POLICY_KEYS = {
    "schema_version",
    "policy_identity",
    "classifier_identity",
    "allowed_paths",
}
EMPTY_DIGEST = hashlib.sha256(b"").hexdigest()
SHA_RE = re.compile(r"[0-9a-f]{40}\Z")


def _nested(value: Any, *keys: str) -> Any:
    for key in keys:
        if not isinstance(value, dict) or key not in value:
            return None
        value = value[key]
    return value


def _normalized_path(value: Any) -> str | None:
    if not isinstance(value, str) or not value or value != value.strip():
        return None
    if value.startswith("/") or "\\" in value or "//" in value:
        return None
    if any(ord(character) < 32 for character in value):
        return None
    parts = value.split("/")
    if any(part in {"", ".", ".."} for part in parts):
        return None
    return value


def _digest(paths: list[str]) -> str:
    canonical = json.dumps(
        sorted(paths), ensure_ascii=False, separators=(",", ":")
    ).encode("utf-8")
    return hashlib.sha256(canonical).hexdigest()


def _identity(event: Any) -> dict[str, Any]:
    event_name = event.get("event_name") if isinstance(event, dict) else None
    pull_request = event.get("pull_request") if isinstance(event, dict) else None
    base_sha = _nested(pull_request, "base", "sha")
    head_sha = _nested(pull_request, "head", "sha")
    changed_files = pull_request.get("changed_files") if isinstance(pull_request, dict) else None
    return {
        "subject": event_name if isinstance(event_name, str) and event_name else "unknown",
        "base_sha": base_sha if isinstance(base_sha, str) else "unknown",
        "head_sha": head_sha if isinstance(head_sha, str) else "unknown",
        "file_count": changed_files if type(changed_files) is int and changed_files >= 0 else 0,
    }


def _decision(
    event: Any,
    *,
    decision: str,
    reason: str,
    paths: list[str] | None = None,
    policy_identity: str = "unavailable",
    policy_digest: str = EMPTY_DIGEST,
    classifier_digest: str = EMPTY_DIGEST,
) -> dict[str, Any]:
    identity = _identity(event)
    normalized_paths = paths or []
    return {
        "schema_version": SCHEMA_VERSION,
        "decision": decision,
        "reason": reason,
        "subject": identity["subject"],
        "file_count": len(normalized_paths) if paths is not None else identity["file_count"],
        "file_digest": _digest(normalized_paths) if paths is not None else EMPTY_DIGEST,
        "base_sha": identity["base_sha"],
        "head_sha": identity["head_sha"],
        "policy_identity": policy_identity,
        "classifier_identity": CLASSIFIER_IDENTITY,
        "policy_digest": policy_digest,
        "classifier_digest": classifier_digest,
    }


def classify(
    event: Any,
    policy: Any,
    pr_before: Any,
    file_pages: Any,
    pr_after: Any,
    *,
    policy_digest: str = EMPTY_DIGEST,
    classifier_digest: str = EMPTY_DIGEST,
) -> dict[str, Any]:
    """Return exactly one typed ``run`` or ``scoped_noop`` decision."""

    if not isinstance(event, dict):
        return _decision(event, decision="run", reason="malformed_event")
    if event.get("event_name") != "pull_request":
        return _decision(event, decision="run", reason="non_pull_request_subject")

    pull_request = event.get("pull_request")
    repository = _nested(event, "repository", "full_name")
    if not isinstance(pull_request, dict) or not isinstance(repository, str) or not repository:
        return _decision(event, decision="run", reason="malformed_pull_request_subject")

    base_sha = _nested(pull_request, "base", "sha")
    head_sha = _nested(pull_request, "head", "sha")
    head_repository = _nested(pull_request, "head", "repo", "full_name")
    number = event.get("number")
    expected_count = pull_request.get("changed_files")
    if (
        not isinstance(base_sha, str)
        or SHA_RE.fullmatch(base_sha) is None
        or not isinstance(head_sha, str)
        or SHA_RE.fullmatch(head_sha) is None
        or type(number) is not int
        or number <= 0
        or type(expected_count) is not int
        or expected_count <= 0
    ):
        return _decision(event, decision="run", reason="invalid_event_identity")
    if not isinstance(head_repository, str) or not head_repository:
        return _decision(event, decision="run", reason="incomplete_head_repository")

    if not isinstance(policy, dict) or set(policy) != POLICY_KEYS:
        return _decision(event, decision="run", reason="invalid_policy_shape")
    policy_identity = policy.get("policy_identity")
    if (
        policy.get("schema_version") != SCHEMA_VERSION
        or policy.get("classifier_identity") != CLASSIFIER_IDENTITY
        or policy_identity != POLICY_IDENTITY
    ):
        return _decision(event, decision="run", reason="invalid_policy_identity")
    allowed_paths = policy.get("allowed_paths")
    if allowed_paths != AUDITED_PATHS:
        return _decision(
            event,
            decision="run",
            reason="invalid_policy_allowlist",
            policy_identity=policy_identity,
            policy_digest=policy_digest,
            classifier_digest=classifier_digest,
        )
    normalized_allowlist = [_normalized_path(path) for path in allowed_paths]
    if any(path is None for path in normalized_allowlist):
        return _decision(
            event,
            decision="run",
            reason="invalid_policy_allowlist",
            policy_identity=policy_identity,
            policy_digest=policy_digest,
            classifier_digest=classifier_digest,
        )

    for observation, label in ((pr_before, "before"), (pr_after, "after")):
        if not isinstance(observation, dict):
            return _decision(
                event,
                decision="run",
                reason=f"invalid_pr_{label}_observation",
                policy_identity=policy_identity,
                policy_digest=policy_digest,
                classifier_digest=classifier_digest,
            )
        if (
            observation.get("number") != number
            or _nested(observation, "base", "sha") != base_sha
            or _nested(observation, "head", "sha") != head_sha
            or observation.get("changed_files") != expected_count
        ):
            return _decision(
                event,
                decision="run",
                reason=f"stale_pr_{label}_observation",
                policy_identity=policy_identity,
                policy_digest=policy_digest,
                classifier_digest=classifier_digest,
            )

    if not isinstance(file_pages, list) or not file_pages:
        return _decision(
            event,
            decision="run",
            reason="invalid_files_pages",
            policy_identity=policy_identity,
            policy_digest=policy_digest,
            classifier_digest=classifier_digest,
        )
    paths: list[str] = []
    for page in file_pages:
        if not isinstance(page, list) or not page:
            return _decision(
                event,
                decision="run",
                reason="invalid_files_page",
                policy_identity=policy_identity,
                policy_digest=policy_digest,
                classifier_digest=classifier_digest,
            )
        for entry in page:
            if not isinstance(entry, dict):
                path = None
            elif "previous_filename" in entry or entry.get("status") not in {
                "added",
                "modified",
                "removed",
            }:
                return _decision(
                    event,
                    decision="run",
                    reason="rename_or_unknown_file_status",
                    policy_identity=policy_identity,
                    policy_digest=policy_digest,
                    classifier_digest=classifier_digest,
                )
            else:
                path = _normalized_path(entry.get("filename"))
            if path is None:
                return _decision(
                    event,
                    decision="run",
                    reason="malformed_changed_path",
                    policy_identity=policy_identity,
                    policy_digest=policy_digest,
                    classifier_digest=classifier_digest,
                )
            paths.append(path)

    if len(paths) != expected_count:
        return _decision(
            event,
            decision="run",
            reason="changed_file_count_mismatch",
            paths=paths,
            policy_identity=policy_identity,
            policy_digest=policy_digest,
            classifier_digest=classifier_digest,
        )
    if len(set(paths)) != len(paths):
        return _decision(
            event,
            decision="run",
            reason="duplicate_changed_path",
            paths=paths,
            policy_identity=policy_identity,
            policy_digest=policy_digest,
            classifier_digest=classifier_digest,
        )

    decision = "scoped_noop" if set(paths).issubset(set(allowed_paths)) else "run"
    reason = "audited_narrative_docs_only" if decision == "scoped_noop" else "full_proof_path"
    return _decision(
        event,
        decision=decision,
        reason=reason,
        paths=paths,
        policy_identity=policy_identity,
        policy_digest=policy_digest,
        classifier_digest=classifier_digest,
    )


def _load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def _run(args: argparse.Namespace) -> dict[str, Any]:
    event: Any = {}
    try:
        event = _load_json(args.event)
    except (OSError, UnicodeError, json.JSONDecodeError):
        return _decision(event, decision="run", reason="malformed_event")
    try:
        policy_bytes = args.policy.read_bytes()
        policy = tomllib.loads(policy_bytes.decode("utf-8"))
        pr_before = _load_json(args.pr_before)
        file_pages = _load_json(args.files)
        pr_after = _load_json(args.pr_after)
    except (OSError, UnicodeError, json.JSONDecodeError, tomllib.TOMLDecodeError):
        return _decision(event, decision="run", reason="unreadable_scope_input")
    try:
        classifier_bytes = Path(__file__).read_bytes()
    except OSError:
        return _decision(event, decision="run", reason="unreadable_classifier_identity")
    return classify(
        event,
        policy,
        pr_before,
        file_pages,
        pr_after,
        policy_digest=hashlib.sha256(policy_bytes).hexdigest(),
        classifier_digest=hashlib.sha256(classifier_bytes).hexdigest(),
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--policy", type=Path, required=True)
    parser.add_argument("--event", type=Path, required=True)
    parser.add_argument("--pr-before", type=Path, required=True)
    parser.add_argument("--files", type=Path, required=True)
    parser.add_argument("--pr-after", type=Path, required=True)
    args = parser.parse_args(argv)
    json.dump(_run(args), sys.stdout, sort_keys=True, separators=(",", ":"))
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
