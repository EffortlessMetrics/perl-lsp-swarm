#!/usr/bin/env python3
"""Validate a release-only exposed-surface disposition projection.

The projection binds release disposition rows to canonical product-authority
rows. It deliberately does not discover product surfaces or reinterpret the
authorities consumed by later adapter work.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any


SCHEMA_VERSION = "exposed_surface_disposition.v1"
SHA_PATTERN = re.compile(r"^[0-9a-f]{40}$")
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")
ISSUE_URL_PATTERN = re.compile(
    r"^https://github\.com/EffortlessMetrics/perl-lsp-swarm/issues/[1-9][0-9]*$"
)
DEFAULT_AUTHORITY_CATALOG = Path(__file__).resolve().parents[1] / "docs/releases/exposed-surface-authorities.v1.json"
DEFAULT_SCHEMA = Path(__file__).resolve().parents[1] / "docs/releases/exposed-surface-disposition.v1.schema.json"


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def _object(value: Any, name: str) -> dict[str, Any]:
    _require(isinstance(value, dict), f"{name} must be an object")
    return value


def _string(value: Any, name: str) -> str:
    _require(isinstance(value, str) and value, f"{name} must be a non-empty string")
    return value


def _sha(value: Any, name: str) -> str:
    text = _string(value, name)
    _require(SHA_PATTERN.fullmatch(text) is not None, f"{name} must be a 40-character lowercase SHA")
    return text


def _sha256(value: Any, name: str) -> str:
    text = _string(value, name)
    _require(SHA256_PATTERN.fullmatch(text) is not None, f"{name} must be a lowercase SHA-256 digest")
    return text


def _schema_enum(schema: dict[str, Any], definition: str, field: str) -> frozenset[str]:
    """Read a closed vocabulary from the JSON schema used by this validator."""

    definitions = _object(schema.get("$defs"), "schema.$defs")
    definition_object = _object(definitions.get(definition), f"schema.$defs.{definition}")
    properties = _object(definition_object.get("properties"), f"schema.$defs.{definition}.properties")
    values = properties.get(field, {}).get("enum")
    _require(
        isinstance(values, list) and values and all(isinstance(value, str) and value for value in values),
        f"schema.$defs.{definition}.properties.{field}.enum must be non-empty strings",
    )
    _require(len(values) == len(set(values)), f"schema.$defs.{definition}.properties.{field}.enum must be unique")
    return frozenset(values)


def _enum(value: Any, allowed: frozenset[str], name: str) -> str:
    text = _string(value, name)
    _require(text in allowed, f"{name} is invalid")
    return text


def _exact_keys(value: Any, expected: set[str], name: str) -> dict[str, Any]:
    data = _object(value, name)
    unexpected = sorted(set(data) - expected)
    missing = sorted(expected - set(data))
    _require(not unexpected, f"{name} has unexpected keys {unexpected}; the projection is closed")
    _require(not missing, f"{name} is missing keys {missing}")
    return data


def _strings(value: Any, name: str, *, minimum: int) -> list[str]:
    _require(isinstance(value, list) and len(value) >= minimum, f"{name} must contain at least {minimum} entries")
    result = [_string(item, f"{name}[{index}]") for index, item in enumerate(value)]
    _require(len(result) == len(set(result)), f"{name} entries must be unique")
    return result


def _validate_evidence(
    value: Any,
    name: str,
    repository_sha: str,
    artifact_profiles: set[str],
    evidence_kinds: frozenset[str],
) -> list[dict[str, str]]:
    _require(isinstance(value, list), f"{name} must be a list")
    evidence: list[dict[str, str]] = []
    fingerprints: set[tuple[str, str, str, str, str]] = set()
    for index, raw in enumerate(value):
        item = _exact_keys(
            raw,
            {"kind", "repository_sha", "artifact_profile", "artifact_sha256", "journey_id", "receipt_ref"},
            f"{name}[{index}]",
        )
        kind = _enum(item.get("kind"), evidence_kinds, f"{name}[{index}].kind")
        subject_sha = _sha(item.get("repository_sha"), f"{name}[{index}].repository_sha")
        _require(
            subject_sha == repository_sha,
            f"{name}[{index}] is bound to a different repository_sha; cross-subject evidence is not transferable",
        )
        profile = _string(item.get("artifact_profile"), f"{name}[{index}].artifact_profile")
        _require(profile in artifact_profiles, f"{name}[{index}].artifact_profile is not declared by the row")
        artifact_sha256 = _sha256(item.get("artifact_sha256"), f"{name}[{index}].artifact_sha256")
        journey_id = _string(item.get("journey_id"), f"{name}[{index}].journey_id")
        receipt_ref = _string(item.get("receipt_ref"), f"{name}[{index}].receipt_ref")
        fingerprint = (kind, profile, artifact_sha256, journey_id, receipt_ref)
        _require(fingerprint not in fingerprints, f"{name} contains a duplicate evidence subject")
        fingerprints.add(fingerprint)
        evidence.append(
            {
                "kind": kind,
                "repository_sha": subject_sha,
                "artifact_profile": profile,
                "artifact_sha256": artifact_sha256,
                "journey_id": journey_id,
                "receipt_ref": receipt_ref,
            }
        )
    return evidence


def _authority_digest(row: dict[str, Any]) -> str:
    payload = json.dumps(row, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    return hashlib.sha256(payload).hexdigest()


def _validate_authorities(value: Any) -> dict[tuple[str, str], dict[str, Any]]:
    entries = _object(value, "authority catalog").get("rows")
    _require(isinstance(entries, list) and entries, "authority catalog.rows must be a non-empty list")
    result: dict[tuple[str, str], dict[str, Any]] = {}
    for index, raw in enumerate(entries):
        item = _object(raw, f"authority catalog.rows[{index}]")
        authority = _string(item.get("authority"), f"authority catalog.rows[{index}].authority")
        row_id = _string(item.get("row_id"), f"authority catalog.rows[{index}].row_id")
        canonical = item.get("row")
        canonical = _object(canonical, f"authority catalog.rows[{index}].row")
        _require(canonical.get("authority") == authority and canonical.get("row_id") == row_id,
                 f"authority catalog.rows[{index}] identity does not match its row")
        key = (authority, row_id)
        _require(key not in result, f"authority catalog duplicates {authority}:{row_id}")
        result[key] = canonical
    return result


def validate_projection(schema: Any, projection: Any, authorities: Any = None) -> None:
    """Raise ``ValueError`` unless projection structure and release laws hold."""

    schema_object = _object(schema, "schema")
    _require(
        schema_object.get("$schema") == "https://json-schema.org/draft/2020-12/schema",
        "schema must declare Draft 2020-12",
    )
    _require(
        schema_object.get("$id")
        == "https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/releases/exposed-surface-disposition.v1.schema.json",
        "schema must retain the canonical exposed-surface disposition identity",
    )

    data = _exact_keys(projection, {"schema", "release", "repository_sha", "rows"}, "projection")
    _require(data.get("schema") == SCHEMA_VERSION, f"projection.schema must be {SCHEMA_VERSION}")
    _string(data.get("release"), "projection.release")
    repository_sha = _sha(data.get("repository_sha"), "projection.repository_sha")
    authority_rows = _validate_authorities(authorities) if authorities is not None else {}
    _require(authorities is not None, "an authority catalog is required to verify surface references")
    dispositions = _schema_enum(schema_object, "row", "disposition")
    effect_classes = _schema_enum(schema_object, "row", "effect_class")
    claim_effects = _schema_enum(schema_object, "row", "claim_effect")
    evidence_kinds = _schema_enum(schema_object, "evidenceSubject", "kind")
    rows = data.get("rows")
    _require(isinstance(rows, list) and rows, "projection.rows must be a non-empty list")

    references: set[tuple[str, str]] = set()
    for index, raw in enumerate(rows):
        row = _exact_keys(
            raw,
            {
                "surface_ref",
                "artifact_profiles",
                "exposure_class",
                "effect_class",
                "default_reachable",
                "opt_in",
                "ordinary_journey",
                "failure_journeys",
                "disposition",
                "owner_issue",
                "evidence_subjects",
                "invalidation_paths",
                "claim_effect",
            },
            f"projection.rows[{index}]",
        )
        source = _exact_keys(row.get("surface_ref"), {"authority", "row_id", "digest"}, f"projection.rows[{index}].surface_ref")
        authority = _string(source.get("authority"), f"projection.rows[{index}].surface_ref.authority")
        row_id = _string(source.get("row_id"), f"projection.rows[{index}].surface_ref.row_id")
        digest = _sha256(source.get("digest"), f"projection.rows[{index}].surface_ref.digest")
        reference = (authority, row_id)
        canonical = authority_rows.get(reference)
        _require(canonical is not None, f"projection.rows[{index}] references unknown canonical authority row {authority}:{row_id}")
        _require(digest == _authority_digest(canonical),
                 f"projection.rows[{index}].surface_ref.digest does not match canonical authority row")
        _require(reference not in references, f"projection rows duplicate canonical authority row {authority}:{row_id}")
        references.add(reference)

        profiles = _strings(row.get("artifact_profiles"), f"projection.rows[{index}].artifact_profiles", minimum=1)
        _string(row.get("exposure_class"), f"projection.rows[{index}].exposure_class")
        _enum(row.get("effect_class"), effect_classes, f"projection.rows[{index}].effect_class")
        _require(type(row.get("default_reachable")) is bool, f"projection.rows[{index}].default_reachable must be boolean")
        _require(type(row.get("opt_in")) is bool, f"projection.rows[{index}].opt_in must be boolean")
        _string(row.get("ordinary_journey"), f"projection.rows[{index}].ordinary_journey")
        _strings(row.get("failure_journeys"), f"projection.rows[{index}].failure_journeys", minimum=1)
        disposition = _enum(row.get("disposition"), dispositions, f"projection.rows[{index}].disposition")
        owner_issue = row.get("owner_issue")
        _require(
            owner_issue is None or (isinstance(owner_issue, str) and ISSUE_URL_PATTERN.fullmatch(owner_issue) is not None),
            f"projection.rows[{index}].owner_issue must be a canonical issue URL or null",
        )
        evidence = _validate_evidence(
            row.get("evidence_subjects"),
            f"projection.rows[{index}].evidence_subjects",
            repository_sha,
            set(profiles),
            evidence_kinds,
        )
        _strings(row.get("invalidation_paths"), f"projection.rows[{index}].invalidation_paths", minimum=1)
        claim_effect = _enum(row.get("claim_effect"), claim_effects, f"projection.rows[{index}].claim_effect")

        evidence_by_profile = {profile: [item for item in evidence if item["artifact_profile"] == profile] for profile in profiles}
        if disposition == "READY":
            _require(claim_effect == "retain", f"projection.rows[{index}].READY must retain its public claim")
            for profile, subjects in evidence_by_profile.items():
                _require(any(item["kind"] == "installed_journey" and item["journey_id"] == row["ordinary_journey"] for item in subjects),
                         f"projection.rows[{index}].READY requires exact installed evidence for {profile} and ordinary journey")
        elif disposition == "BOUNDED_PREVIEW":
            _require(claim_effect == "limit", f"projection.rows[{index}].BOUNDED_PREVIEW must limit its public claim")
            for profile, subjects in evidence_by_profile.items():
                _require(any(item["kind"] == "installed_journey" and item["journey_id"] == row["ordinary_journey"] for item in subjects),
                         f"projection.rows[{index}].BOUNDED_PREVIEW requires exact installed evidence for {profile} and ordinary journey")
                _require(any(item["kind"] == "refusal_boundary" and item["journey_id"] in row["failure_journeys"] for item in subjects),
                         f"projection.rows[{index}].BOUNDED_PREVIEW requires refusal-boundary evidence for {profile} and a failure journey")
        elif disposition == "DISABLED":
            _require(claim_effect == "remove_or_withhold", f"projection.rows[{index}].DISABLED must remove or withhold its claim")
            _require(row["default_reachable"] is False, f"projection.rows[{index}].DISABLED cannot remain default reachable")
            for profile, subjects in evidence_by_profile.items():
                _require(any(item["kind"] == "artifact_absence" for item in subjects),
                         f"projection.rows[{index}].DISABLED requires artifact-absence evidence for {profile}")
        else:
            _require(claim_effect == "remove_or_withhold", f"projection.rows[{index}].{disposition} must remove or withhold its claim")
            _require(owner_issue is not None, f"projection.rows[{index}].{disposition} requires an owning issue")


def canonical_bytes(projection: Any) -> str:
    return json.dumps(projection, sort_keys=True, indent=2, ensure_ascii=False) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--schema",
        type=Path,
        default=DEFAULT_SCHEMA,
    )
    parser.add_argument("--projection", type=Path, required=True)
    parser.add_argument("--authority-catalog", type=Path, default=DEFAULT_AUTHORITY_CATALOG)
    args = parser.parse_args(argv)
    try:
        schema = json.loads(args.schema.read_text(encoding="utf-8"))
        text = args.projection.read_text(encoding="utf-8")
        projection = json.loads(text)
        authorities = json.loads(args.authority_catalog.read_text(encoding="utf-8"))
        validate_projection(schema, projection, authorities)
        _require(text == canonical_bytes(projection), "projection bytes are not canonical; identical inputs must generate identical bytes")
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"exposed-surface disposition validation failed: {error}", file=sys.stderr)
        return 1
    print(f"exposed-surface disposition validation passed: {args.projection}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
