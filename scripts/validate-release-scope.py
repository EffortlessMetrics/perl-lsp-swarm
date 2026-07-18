#!/usr/bin/env python3
"""Validate the cross-field invariants of a release admission receipt.

JSON Schema validates each field independently.  This checker owns the
relational rules that Draft 2020-12 cannot express for this receipt, while
keeping final-freeze and live GitHub queries out of the admission validator.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import datetime
from pathlib import Path
from typing import Any


SHA_PATTERN = re.compile(r"^[0-9a-f]{40}$")
PULL_PATTERN = re.compile(r"^https://github\.com/EffortlessMetrics/perl-lsp-swarm/pull/(\d+)$")
ISSUE_PATTERN = re.compile(r"^https://github\.com/EffortlessMetrics/perl-lsp-swarm/issues/\d+$")
OBSERVATION_QUERY = (
    "gh pr list --repo EffortlessMetrics/perl-lsp-swarm --state open --limit 100 --json number"
)


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
    _require(isinstance(value, str) and SHA_PATTERN.fullmatch(value) is not None, f"{name} must be a 40-character lowercase SHA")


def _utc_timestamp(value: Any, name: str) -> datetime:
    text = _string(value, name)
    _require(text.endswith("Z"), f"{name} must use UTC and end in Z")
    try:
        return datetime.fromisoformat(text[:-1] + "+00:00")
    except ValueError as error:
        raise ValueError(f"{name} must be an ISO-8601 UTC timestamp") from error


def validate_scope(schema: Any, receipt: Any) -> None:
    """Raise ``ValueError`` unless schema metadata and receipt invariants hold."""

    schema_object = _object(schema, "schema")
    _require(schema_object.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "schema must declare Draft 2020-12")
    data = _object(receipt, "receipt")
    _require(data.get("schema") == 1, "receipt.schema must be 1")
    _require(data.get("release") == "0.18.0", "receipt.release must be 0.18.0")
    _require(data.get("track") == "public-beta", "receipt.track must be public-beta")
    _require(data.get("phase") == "admission-frozen", "receipt.phase must be admission-frozen")
    _sha(data.get("observation_sha"), "observation_sha")
    observed_at = _utc_timestamp(data.get("observed_at_utc"), "observed_at_utc")
    _require(data.get("frozen_swarm_sha") is None, "admission receipts cannot record a frozen swarm SHA")

    freeze_rules = _object(data.get("freeze_rules"), "freeze_rules")
    _require(freeze_rules.get("features_closed") is True, "freeze_rules.features_closed must be true")
    _require(
        freeze_rules.get("late_arrivals_default") == "post-0.18-unless-admitted-as-release-blocker",
        "freeze_rules.late_arrivals_default must preserve the post-0.18 default",
    )
    _require(freeze_rules.get("superseded_requires_close_proof") is True, "freeze_rules.superseded_requires_close_proof must be true")
    _require(
        freeze_rules.get("acceptance_invalidation") == "any-product-mutation-requires-fresh-installed-acceptance",
        "freeze_rules.acceptance_invalidation must preserve the installed-acceptance invalidation rule",
    )

    classification_method = _object(data.get("classification_method"), "classification_method")
    _require(
        "NOT_PROVEN" in _string(classification_method.get("base_and_ancestry"), "classification_method.base_and_ancestry"),
        "classification_method must not claim unproven head ancestry",
    )

    snapshot = _object(data.get("queue_snapshot"), "queue_snapshot")
    _require(snapshot.get("query") == OBSERVATION_QUERY, "queue_snapshot.query must pin the repository and bounded query")
    query_limit = snapshot.get("query_limit")
    observed_count = snapshot.get("observed_open_count")
    receipt_count = snapshot.get("receipt_count")
    numbers = snapshot.get("observed_numbers")
    _require(isinstance(query_limit, int) and query_limit > 0, "query_limit must be a positive integer")
    _require(isinstance(observed_count, int) and observed_count >= 0, "observed_open_count must be a non-negative integer")
    _require(isinstance(receipt_count, int) and receipt_count >= 0, "receipt_count must be a non-negative integer")
    _require(isinstance(numbers, list) and numbers, "observed_numbers must be a non-empty list")
    _require(numbers == sorted(numbers), "observed_numbers must be sorted")
    _require(len(numbers) == len(set(numbers)), "observed_numbers must be unique")
    _require(query_limit >= observed_count, "query_limit must cover observed_open_count")
    _require(snapshot.get("set_equality") is True, "queue_snapshot.set_equality must be true")

    items = data.get("items")
    _require(isinstance(items, list) and items, "items must be a non-empty list")
    _require(receipt_count == len(items) == observed_count, "receipt_count, items.length, and observed_open_count must agree")
    item_numbers: list[int] = []
    for index, item_value in enumerate(items):
        item = _object(item_value, f"items[{index}]")
        number = item.get("number")
        _require(isinstance(number, int) and number > 0, f"items[{index}].number must be positive")
        item_numbers.append(number)
        _sha(item.get("head_sha"), f"items[{index}].head_sha")
        match = PULL_PATTERN.fullmatch(_string(item.get("url"), f"items[{index}].url"))
        _require(match is not None and int(match.group(1)) == number, f"items[{index}].url must match its PR number")
        follow_up = item.get("follow_up_issue")
        _require(follow_up is None or ISSUE_PATTERN.fullmatch(follow_up) is not None, f"items[{index}].follow_up_issue must be a repository issue URL or null")
        disposition = item.get("disposition")
        _require(disposition in {"0.18-blocker", "0.18-candidate", "post-0.18", "superseded"}, f"items[{index}].disposition is invalid")
        if disposition == "post-0.18":
            _require(follow_up is not None, f"items[{index}].post-0.18 disposition requires follow_up_issue")
        if disposition == "0.18-blocker":
            _require(isinstance(item.get("owner"), str) and item["owner"], f"items[{index}].0.18-blocker requires owner")
            _require(isinstance(item.get("acceptance"), list) and item["acceptance"], f"items[{index}].0.18-blocker requires acceptance")
            _require(isinstance(item.get("proof"), list) and item["proof"], f"items[{index}].0.18-blocker requires proof")
        if disposition == "superseded":
            _require(isinstance(item.get("close_ready"), bool), f"items[{index}].superseded requires boolean close_ready")

    _require(len(item_numbers) == len(set(item_numbers)), "items.number must be unique")
    _require(sorted(item_numbers) == numbers, "items.number must exactly match queue_snapshot.observed_numbers as a set")
    _require(observed_at.tzinfo is not None, "observed_at_utc must include a timezone")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--schema", type=Path, default=Path("docs/releases/release-scope.schema.json"))
    parser.add_argument("--receipt", type=Path, default=Path("docs/releases/v0.18.0-scope.json"))
    args = parser.parse_args(argv)
    try:
        schema = json.loads(args.schema.read_text(encoding="utf-8"))
        receipt = json.loads(args.receipt.read_text(encoding="utf-8"))
        validate_scope(schema, receipt)
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"release scope validation failed: {error}", file=sys.stderr)
        return 1
    print(f"release scope validation passed: {args.receipt}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
