#!/usr/bin/env python3
"""Validate the 0.18.0-rc.1 product-denominator intake receipt.

The checker owns every relational rule the RC intake contract requires:
exactly-one-disposition coverage of the observed queue, disjoint dispositions,
merged work represented as landed tree state (never candidate heads), bounded
blocker/included shapes, closed feature intake, a fixed allowed-change class
list, canonical deterministic bytes, and an admission-time null product SHA.
Live GitHub queries and the post-merge branch pin remain outside this
validator; they are recorded on issue #12228 after merge.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


SCHEMA_VERSION = "v0.18_rc_intake.v1"
RELEASE = "0.18.0-rc.1"
TRACK = "public-alpha-release-candidate"
RELEASE_BRANCH = "release/0.18"
OBSERVATION_QUERY = (
    "gh pr list --repo EffortlessMetrics/perl-lsp-swarm "
    "--state open --limit 100 --json number,title,headRefName,updatedAt,isDraft"
)
QUERY_LIMIT = 100

ALLOWED_CHANGE_CLASSES = [
    "rc-reproduced-blocker",
    "deterministic-release-preparation",
    "release-lineage-reconciliation",
    "release-proof",
]
PRODUCT_INVALIDATION = [
    "freeze",
    "preparation",
    "reconciliation",
    "projection",
    "sync",
    "candidate",
    "approval",
]
METADATA_INVALIDATION = [
    "preparation",
    "projection",
    "sync",
    "candidate",
    "approval",
]
PUBLIC_CLAIM_BOUNDARY = {
    "primary_editor": "vscode",
    "lsp": "daily-driver-public-alpha",
    "dap": "preview",
    "formatting": "whole-document-only",
    "remote_ai": "fail-closed-without-trusted-adapter",
}

DISPOSITION_LISTS = [
    "included_prs",
    "required_blockers",
    "excluded_post_rc",
    "superseded_for_release",
    "not_release_relevant",
    "not_proven",
]

SHA_PATTERN = re.compile(r"^[0-9a-f]{40}$")


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def _object(value: Any, name: str) -> dict[str, Any]:
    _require(isinstance(value, dict), f"{name} must be an object")
    return value


def _string(value: Any, name: str) -> str:
    _require(isinstance(value, str) and value, f"{name} must be a non-empty string")
    return value


def _sha(value: Any, name: str) -> None:
    _require(
        isinstance(value, str) and SHA_PATTERN.fullmatch(value) is not None,
        f"{name} must be a 40-character lowercase SHA",
    )


def _number_entries(value: Any, name: str) -> list[dict[str, Any]]:
    _require(isinstance(value, list), f"{name} must be a list")
    for index, entry in enumerate(value):
        entry_object = _object(entry, f"{name}[{index}]")
        number = entry_object.get("number")
        _require(
            isinstance(number, int) and not isinstance(number, bool) and number > 0,
            f"{name}[{index}].number must be a positive integer",
        )
        _require("reason" in entry_object, f"{name}[{index}] requires reason")
        _string(entry_object.get("reason"), f"{name}[{index}].reason")
    return value


def validate_intake(receipt: Any) -> None:
    """Raise ``ValueError`` unless the RC intake receipt holds together."""

    data = _object(receipt, "receipt")
    _require(data.get("schema_version") == SCHEMA_VERSION, "schema_version must be v0.18_rc_intake.v1")
    _require(data.get("release") == RELEASE, "release must be 0.18.0-rc.1")
    _require(data.get("track") == TRACK, "track must be public-alpha-release-candidate")
    _require(data.get("release_branch") == RELEASE_BRANCH, "release_branch must be release/0.18")

    _sha(data.get("observation_sha"), "observation_sha")
    _require(
        isinstance(data.get("observed_at_utc"), str) and data["observed_at_utc"].endswith("Z"),
        "observed_at_utc must be a UTC timestamp ending in Z",
    )
    _require(
        data.get("frozen_product_sha") is None,
        "admission receipts cannot record a frozen product SHA; the denominator is the merge commit that introduces this receipt",
    )

    snapshot = _object(data.get("queue_snapshot"), "queue_snapshot")
    _require(snapshot.get("query") == OBSERVATION_QUERY, "queue_snapshot.query must pin the repository and bounded query")
    _require(snapshot.get("query_limit") == QUERY_LIMIT, "queue_snapshot.query_limit must be 100")
    observed_count = snapshot.get("observed_open_count")
    receipt_count = snapshot.get("receipt_count")
    numbers = snapshot.get("observed_numbers")
    _require(isinstance(observed_count, int) and observed_count >= 0, "observed_open_count must be a non-negative integer")
    _require(isinstance(receipt_count, int) and receipt_count >= 0, "receipt_count must be a non-negative integer")
    _require(isinstance(numbers, list) and numbers, "observed_numbers must be a non-empty list")
    _require(all(isinstance(item, int) and item > 0 for item in numbers), "observed_numbers must be positive integers")
    _require(numbers == sorted(numbers), "observed_numbers must be sorted")
    _require(len(numbers) == len(set(numbers)), "observed_numbers must be unique")
    _require(QUERY_LIMIT > observed_count, "query_limit must leave headroom above observed_open_count")
    _require(snapshot.get("set_equality") is True, "queue_snapshot.set_equality must be true")

    already = data.get("already_included")
    _require(isinstance(already, list), "already_included must be a list")
    already_numbers: list[int] = []
    for index, entry in enumerate(already):
        entry_object = _object(entry, f"already_included[{index}]")
        number = entry_object.get("number")
        _require(isinstance(number, int) and number > 0, f"already_included[{index}].number must be a positive integer")
        already_numbers.append(number)
        _sha(entry_object.get("landed_sha"), f"already_included[{index}].landed_sha")
        note = entry_object.get("note")
        _require(isinstance(note, str) and note, f"already_included[{index}].note must be a non-empty string")
    _require(len(already_numbers) == len(set(already_numbers)), "already_included.numbers must be unique")

    included = _number_entries(data.get("included_prs"), "included_prs")
    blockers = data.get("required_blockers")
    _require(isinstance(blockers, list), "required_blockers must be a list")
    blocker_numbers: list[int] = []
    for index, entry in enumerate(blockers):
        entry_object = _object(entry, f"required_blockers[{index}]")
        number = entry_object.get("number")
        _require(isinstance(number, int) and number > 0, f"required_blockers[{index}].number must be a positive integer")
        blocker_numbers.append(number)
        _string(entry_object.get("owner"), f"required_blockers[{index}].owner")
        _string(
            entry_object.get("repair_or_withdrawal"),
            f"required_blockers[{index}].repair_or_withdrawal",
        )
        _string(entry_object.get("proof_path"), f"required_blockers[{index}].proof_path")

    excluded = _number_entries(data.get("excluded_post_rc"), "excluded_post_rc")
    superseded = _number_entries(data.get("superseded_for_release"), "superseded_for_release")
    irrelevant = _number_entries(data.get("not_release_relevant"), "not_release_relevant")
    unproven = _number_entries(data.get("not_proven"), "not_proven")

    buckets: dict[str, set[int]] = {
        "included_prs": {entry["number"] for entry in included},
        "required_blockers": set(blocker_numbers),
        "excluded_post_rc": {entry["number"] for entry in excluded},
        "superseded_for_release": {entry["number"] for entry in superseded},
        "not_release_relevant": {entry["number"] for entry in irrelevant},
        "not_proven": {entry["number"] for entry in unproven},
    }

    observed_set = set(numbers)
    covered: set[int] = set()
    for name, members in buckets.items():
        overlap = covered & members
        _require(not overlap, f"{name} overlaps an earlier disposition for PRs {sorted(overlap)}")
        covered |= members
    missing = observed_set - covered - set(already_numbers)
    _require(not missing, f"observed release-affecting queue omitted from every disposition: {sorted(missing)}")
    extra = covered - observed_set
    _require(not extra, f"dispositions reference PRs outside the observed open queue: {sorted(extra)}")

    known_limitations = data.get("known_limitations")
    _require(isinstance(known_limitations, list) and known_limitations, "known_limitations must be a non-empty list")
    for index, limitation in enumerate(known_limitations):
        _string(limitation, f"known_limitations[{index}]")

    boundary = _object(data.get("public_claim_boundary"), "public_claim_boundary")
    _require(boundary == PUBLIC_CLAIM_BOUNDARY, "public_claim_boundary must match the declared RC claim exactly")

    classes = data.get("allowed_change_classes")
    _require(classes == ALLOWED_CHANGE_CLASSES, "allowed_change_classes must equal the bounded release class list")

    invalidation = _object(data.get("invalidation"), "invalidation")
    _require(invalidation.get("product_change") == PRODUCT_INVALIDATION, "invalidation.product_change must match the declared order")
    _require(invalidation.get("metadata_change") == METADATA_INVALIDATION, "invalidation.metadata_change must match the declared order")

    _require(data.get("feature_intake_closed") is True, "feature_intake_closed must be true at admission")
    _require(data.get("issue_closures_required") == [], "issue_closures_required must remain empty")


def canonical_bytes(receipt: Any) -> str:
    return json.dumps(receipt, sort_keys=True, indent=2, ensure_ascii=False) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt", type=Path, default=Path("docs/releases/v0.18.0-rc.1-intake.json"))
    args = parser.parse_args(argv)
    try:
        text = args.receipt.read_text(encoding="utf-8")
        receipt = json.loads(text)
        validate_intake(receipt)
        _require(text == canonical_bytes(receipt), "receipt bytes are not canonical; identical inputs must generate identical bytes")
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"rc intake validation failed: {error}", file=sys.stderr)
        return 1
    print(f"rc intake validation passed: {args.receipt}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
