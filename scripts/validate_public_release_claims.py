#!/usr/bin/env python3
"""Validate a candidate-bound public_release_claims.v1 catalog."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


SCHEMA_VERSION = "public_release_claims.v1"
SHA40 = re.compile(r"^[0-9a-f]{40}$")
TOPOLOGY_DIGEST = re.compile(r"^sha256:[0-9a-f]{64}$")
STATUS_REQUIRING_LIMITATION = {"bounded", "blocked", "not_proven"}


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def _string(value: Any, name: str) -> str:
    _require(isinstance(value, str) and value, f"{name} must be a non-empty string")
    return value


def validate_claims(catalog: dict[str, Any]) -> None:
    _require(catalog.get("schema_version") == SCHEMA_VERSION, "schema_version is invalid")
    _require(catalog.get("release") == "0.18.0", "release must be 0.18.0")
    _require(catalog.get("track") == "public-beta", "track must be public-beta")
    subject_sha = _string(catalog.get("subject_sha"), "subject_sha")
    _require(SHA40.fullmatch(subject_sha) is not None, "subject_sha must be a lowercase 40-character SHA")
    topology_digest = _string(catalog.get("topology_digest"), "topology_digest")
    _require(TOPOLOGY_DIGEST.fullmatch(topology_digest) is not None, "topology_digest must be sha256:<64 lowercase hex>")

    claims = catalog.get("claims")
    _require(isinstance(claims, list) and claims, "claims must be a non-empty array")
    ids: list[str] = []
    for index, raw_claim in enumerate(claims):
        name = f"claims[{index}]"
        _require(isinstance(raw_claim, dict), f"{name} must be an object")
        claim = raw_claim
        claim_id = _string(claim.get("id"), f"{name}.id")
        _require(re.fullmatch(r"[a-z0-9]+(?:[._-][a-z0-9]+)+", claim_id) is not None, f"{name}.id is invalid")
        ids.append(claim_id)
        surfaces = claim.get("surfaces")
        _require(isinstance(surfaces, list) and surfaces, f"{name}.surfaces must be non-empty")
        _require(len(surfaces) == len(set(surfaces)), f"{name}.surfaces must be unique")
        for surface in surfaces:
            _string(surface, f"{name}.surfaces[]")
        _require(claim.get("audience") == "user", f"{name}.audience must be user")
        _string(claim.get("text_or_command"), f"{name}.text_or_command")
        authority = claim.get("authority")
        _require(authority in {"release_topology", "installed_transition", "experience_contract", "api_audit", "release_integrity"}, f"{name}.authority is invalid")
        evidence_refs = claim.get("evidence_refs")
        _require(isinstance(evidence_refs, list) and evidence_refs, f"{name}.evidence_refs must be non-empty")
        _require(len(evidence_refs) == len(set(evidence_refs)), f"{name}.evidence_refs must be unique")
        for reference in evidence_refs:
            _string(reference, f"{name}.evidence_refs[]")
        status = claim.get("status")
        _require(status in {"proven", "bounded", "blocked", "not_proven"}, f"{name}.status is invalid")
        _require(claim.get("public_context") in {"swarm", "publication", "both"}, f"{name}.public_context is invalid")
        limitation = claim.get("limitation")
        if status in STATUS_REQUIRING_LIMITATION:
            _require(isinstance(limitation, str) and limitation.strip(), f"{name}.{status} requires a limitation")
        elif limitation is not None:
            _require(isinstance(limitation, str) and limitation.strip(), f"{name}.limitation must be null or non-empty")

    _require(len(ids) == len(set(ids)), "claims.id values must be unique")
    _require(ids == sorted(ids), "claims must be sorted by id for deterministic catalogs")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        catalog = json.loads(args.catalog.read_text(encoding="utf-8"))
        if not isinstance(catalog, dict):
            raise ValueError("catalog must be an object")
        validate_claims(catalog)
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"public-release-claims: invalid: {error}", file=sys.stderr)
        return 1
    print(f"public-release-claims: valid ({len(catalog['claims'])} claims)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
