#!/usr/bin/env python3
"""Validate a release admission receipt against its schema and cross-field rules.

The standard-library schema subset catches structural drift, and this checker
owns the relational rules that Draft 2020-12 cannot express for this receipt.
Final-freeze and live GitHub queries remain outside the admission validator.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any


SHA_PATTERN = re.compile(r"^[0-9a-f]{40}$")
PULL_PATTERN = re.compile(r"^https://github\.com/EffortlessMetrics/perl-lsp-swarm/pull/(\d+)$")
ISSUE_PATTERN = re.compile(r"^https://github\.com/EffortlessMetrics/perl-lsp-swarm/issues/\d+$")
OBSERVATION_QUERY = (
    "gh pr list --repo EffortlessMetrics/perl-lsp-swarm --state open --limit 100 --json number"
)
QUERY_LIMIT_PATTERN = re.compile(r"(?:^|\s)--limit\s+(\d+)(?:\s|$)")


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


def _schema_ref(root: dict[str, Any], reference: str) -> dict[str, Any]:
    _require(reference.startswith("#/$defs/"), f"unsupported schema reference: {reference}")
    name = reference.removeprefix("#/$defs/")
    definition = root.get("$defs", {}).get(name)
    _require(isinstance(definition, dict), f"schema reference is missing: {reference}")
    return definition


def _schema_type_matches(value: Any, expected: str) -> bool:
    if expected == "object":
        return isinstance(value, dict)
    if expected == "array":
        return isinstance(value, list)
    if expected == "string":
        return isinstance(value, str)
    if expected == "integer":
        return isinstance(value, int) and not isinstance(value, bool)
    if expected == "boolean":
        return isinstance(value, bool)
    if expected == "null":
        return value is None
    if expected == "number":
        return isinstance(value, (int, float)) and not isinstance(value, bool)
    return False


def _schema_errors(node: Any, value: Any, path: str, root: dict[str, Any]) -> list[str]:
    """Validate the Draft 2020-12 subset used by the release-scope schema."""

    if not isinstance(node, dict):
        return [f"{path}: schema node must be an object"]
    if "$ref" in node:
        return _schema_errors(_schema_ref(root, node["$ref"]), value, path, root)

    errors: list[str] = []
    if "const" in node and value != node["const"]:
        errors.append(f"{path}: expected const {node['const']!r}")
    if "enum" in node and value not in node["enum"]:
        errors.append(f"{path}: expected one of {node['enum']!r}")

    expected_type = node.get("type")
    if expected_type is not None:
        types = expected_type if isinstance(expected_type, list) else [expected_type]
        if not any(_schema_type_matches(value, candidate) for candidate in types):
            return errors + [f"{path}: expected type {expected_type!r}"]

    if isinstance(value, str):
        if "minLength" in node and len(value) < node["minLength"]:
            errors.append(f"{path}: string is shorter than minLength")
        if "pattern" in node and re.search(node["pattern"], value) is None:
            errors.append(f"{path}: string does not match pattern")
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if "minimum" in node and value < node["minimum"]:
            errors.append(f"{path}: number is below minimum")
    if isinstance(value, list):
        if "minItems" in node and len(value) < node["minItems"]:
            errors.append(f"{path}: array is shorter than minItems")
        if "maxItems" in node and len(value) > node["maxItems"]:
            errors.append(f"{path}: array is longer than maxItems")
        if node.get("uniqueItems"):
            encoded = [json.dumps(item, sort_keys=True, separators=(",", ":")) for item in value]
            if len(encoded) != len(set(encoded)):
                errors.append(f"{path}: array items must be unique")
        if "items" in node:
            for index, item in enumerate(value):
                errors.extend(_schema_errors(node["items"], item, f"{path}[{index}]", root))
        if "contains" in node and not any(not _schema_errors(node["contains"], item, path, root) for item in value):
            errors.append(f"{path}: array does not contain a matching item")
    if isinstance(value, dict):
        properties = node.get("properties", {})
        for required in node.get("required", []):
            if required not in value:
                errors.append(f"{path}: missing required property {required!r}")
        if node.get("additionalProperties") is False:
            for key in value:
                if key not in properties:
                    errors.append(f"{path}: unexpected property {key!r}")
        for key, child in properties.items():
            if key in value:
                errors.extend(_schema_errors(child, value[key], f"{path}.{key}", root))

    if "oneOf" in node:
        matches = sum(not _schema_errors(option, value, path, root) for option in node["oneOf"])
        if matches != 1:
            errors.append(f"{path}: expected exactly one matching oneOf branch")
    if "allOf" in node:
        for child in node["allOf"]:
            errors.extend(_schema_errors(child, value, path, root))
    if "if" in node and not _schema_errors(node["if"], value, path, root) and "then" in node:
        errors.extend(_schema_errors(node["then"], value, path, root))
    return errors


def _validate_against_schema(schema: Any, receipt: Any) -> None:
    schema_object = _object(schema, "schema")
    errors = _schema_errors(schema_object, receipt, "$", schema_object)
    if errors:
        raise ValueError(f"receipt does not satisfy schema: {errors[0]}")


def validate_scope(schema: Any, receipt: Any) -> None:
    """Raise ``ValueError`` unless schema metadata and receipt invariants hold."""

    schema_object = _object(schema, "schema")
    _require(schema_object.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "schema must declare Draft 2020-12")
    data = _object(receipt, "receipt")
    _validate_against_schema(schema_object, data)
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
        classification_method.get("ancestry_status") == "not-proven",
        "classification_method.ancestry_status must be not-proven",
    )

    snapshot = _object(data.get("queue_snapshot"), "queue_snapshot")
    _require(snapshot.get("query") == OBSERVATION_QUERY, "queue_snapshot.query must pin the repository and bounded query")
    query_limit = snapshot.get("query_limit")
    observed_count = snapshot.get("observed_open_count")
    receipt_count = snapshot.get("receipt_count")
    numbers = snapshot.get("observed_numbers")
    query_match = QUERY_LIMIT_PATTERN.search(OBSERVATION_QUERY)
    _require(query_match is not None, "OBSERVATION_QUERY must declare a numeric --limit")
    expected_query_limit = int(query_match.group(1))
    _require(query_limit == expected_query_limit, "query_limit must match the pinned observation query")
    _require(isinstance(observed_count, int) and observed_count >= 0, "observed_open_count must be a non-negative integer")
    _require(isinstance(receipt_count, int) and receipt_count >= 0, "receipt_count must be a non-negative integer")
    _require(isinstance(numbers, list) and numbers, "observed_numbers must be a non-empty list")
    _require(numbers == sorted(numbers), "observed_numbers must be sorted")
    _require(len(numbers) == len(set(numbers)), "observed_numbers must be unique")
    _require(query_limit > observed_count, "query_limit must leave headroom above observed_open_count")
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
        _require(
            follow_up is None or (isinstance(follow_up, str) and ISSUE_PATTERN.fullmatch(follow_up) is not None),
            f"items[{index}].follow_up_issue must be a repository issue URL or null",
        )
        disposition = item.get("disposition")
        _require(disposition in {"0.18-blocker", "0.18-candidate", "post-0.18", "superseded"}, f"items[{index}].disposition is invalid")
        if disposition == "post-0.18":
            _require(follow_up is not None, f"items[{index}].post-0.18 disposition requires follow_up_issue")
        if disposition == "0.18-blocker":
            _require(isinstance(item.get("owner"), str) and item["owner"], f"items[{index}].0.18-blocker requires owner")
            _require(isinstance(item.get("acceptance"), list) and item["acceptance"], f"items[{index}].0.18-blocker requires acceptance")
            _require(isinstance(item.get("proof"), list) and item["proof"], f"items[{index}].0.18-blocker requires proof")
        if disposition == "0.18-candidate":
            _require(isinstance(item.get("owner"), str) and item["owner"], f"items[{index}].0.18-candidate requires owner")
            _require(isinstance(item.get("acceptance"), list) and item["acceptance"], f"items[{index}].0.18-candidate requires acceptance")
            _require(isinstance(item.get("proof"), list) and item["proof"], f"items[{index}].0.18-candidate requires proof")
            _require(item.get("unresolved_threads") == 0, f"items[{index}].0.18-candidate requires unresolved_threads == 0")
            checks = item.get("checks") if isinstance(item.get("checks"), dict) else {}
            _require(checks.get("failed") == 0, f"items[{index}].0.18-candidate requires checks.failed == 0")
            _require(checks.get("pending") == 0, f"items[{index}].0.18-candidate requires checks.pending == 0")
        if disposition == "superseded":
            _require(isinstance(item.get("close_ready"), bool), f"items[{index}].superseded requires boolean close_ready")

    _require(len(item_numbers) == len(set(item_numbers)), "items.number must be unique")
    _require(sorted(item_numbers) == numbers, "items.number must exactly match queue_snapshot.observed_numbers as a set")
    _require(observed_at.utcoffset() == timedelta(0), "observed_at_utc must use UTC")


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
