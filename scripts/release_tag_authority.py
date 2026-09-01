#!/usr/bin/env python3
"""Fail closed unless a release tag is immutable and names the exact subject."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

HEX40 = re.compile(r"^[0-9a-f]{40}$")


class TagAuthorityError(ValueError):
    """Release-tag immutability or exact currentness is NOT_PROVEN."""


def load(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise TagAuthorityError(f"NOT_PROVEN: invalid {label}: {path}") from error
    if not isinstance(value, dict):
        raise TagAuthorityError(f"NOT_PROVEN: {label} is not an object")
    return value


def validate_ruleset(value: dict[str, Any], repository: str | None = None) -> None:
    if (
        value.get("name") != "release-tags"
        or value.get("target") != "tag"
        or value.get("source_type") != "Repository"
        or value.get("enforcement") != "active"
        or (repository is not None and value.get("source") != repository)
    ):
        raise TagAuthorityError("NOT_PROVEN: release-tag ruleset identity is inactive or changed")
    conditions = value.get("conditions")
    ref_name = conditions.get("ref_name") if isinstance(conditions, dict) else None
    if not isinstance(ref_name, dict) or ref_name.get("exclude") != [] or "refs/tags/v*" not in ref_name.get("include", []):
        raise TagAuthorityError("NOT_PROVEN: release-tag ruleset does not cover refs/tags/v*")
    rules = value.get("rules")
    rule_types = {
        row.get("type") for row in rules if isinstance(row, dict)
    } if isinstance(rules, list) else set()
    # GitHub's non_fast_forward rule rejects only non-fast-forward movement;
    # lightweight tags can still move forward. Exact-SHA authority therefore
    # requires the distinct update restriction as well as deletion protection.
    if not {"deletion", "update"}.issubset(rule_types):
        raise TagAuthorityError(
            "NOT_PROVEN: release-tag ruleset lacks update/deletion immutability"
        )
    if value.get("bypass_actors") != [] or value.get("current_user_can_bypass") != "never":
        raise TagAuthorityError("NOT_PROVEN: release-tag authority is bypassable")


def validate_ref(value: dict[str, Any], tag: str, source_sha: str) -> None:
    if not HEX40.fullmatch(source_sha) or not tag.startswith("v"):
        raise TagAuthorityError("NOT_PROVEN: release tag subject is malformed")
    obj = value.get("object")
    if value.get("ref") != f"refs/tags/{tag}" or not isinstance(obj, dict):
        raise TagAuthorityError("NOT_PROVEN: release tag ref identity mismatch")
    if obj.get("type") != "commit" or obj.get("sha") != source_sha:
        raise TagAuthorityError("NOT_PROVEN: release tag currentness moved from exact source")


def validate(
    ruleset: dict[str, Any], ref: dict[str, Any] | None, tag: str, source_sha: str,
    repository: str | None = None,
) -> None:
    validate_ruleset(ruleset, repository)
    if ref is not None:
        validate_ref(ref, tag, source_sha)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ruleset", type=Path, required=True)
    parser.add_argument("--ref", type=Path)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--repository")
    args = parser.parse_args()
    try:
        validate(
            load(args.ruleset, "release-tag ruleset"),
            load(args.ref, "release-tag ref") if args.ref else None,
            args.tag,
            args.source_sha,
            args.repository,
        )
    except TagAuthorityError as error:
        print(f"release tag authority: {error}", file=sys.stderr)
        return 1
    if args.ref is None:
        # Ruleset-only preflight: exact tag currentness was NOT checked, so
        # this must not be mistaken for the authoritative PROVEN verdict.
        print("release tag authority: RULESET PREFLIGHT OK (no ref validated)")
        return 0
    print("release tag authority: PROVEN")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
