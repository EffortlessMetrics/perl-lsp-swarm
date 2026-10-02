#!/usr/bin/env python3
"""Fail closed unless the governed workflow proved this exact Rust Small subject."""

from __future__ import annotations

import datetime
import json
import os
import re
import sys
from pathlib import Path
from typing import Mapping


def unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    """Reject ambiguous duplicate keys instead of accepting the last value."""
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate evidence field: {key}")
        result[key] = value
    return result


def validate_evidence(environment: Mapping[str, str]) -> dict[str, str]:
    """Require successful entry/completion and bind the central result to its caller."""
    if environment.get("EM_CI_CALL_RESULT") != "success":
        raise ValueError("governed workflow did not succeed")
    return validate_proof_evidence(environment)


def validate_proof_evidence(environment: Mapping[str, str]) -> dict[str, str]:
    """Validate selected-proof evidence inside a still-running governed policy job.

    The caller owns the successful evaluator prerequisite. This does not certify
    that the reusable workflow, its policy or its receipt has completed.
    """
    evidence = json.loads(
        environment.get("EM_CI_EVIDENCE", ""), object_pairs_hook=unique_object
    )
    if not isinstance(evidence, dict) or any(
        not isinstance(value, str) for value in evidence.values()
    ):
        raise ValueError("evidence must be an object of string fields")
    expected = {
        "schema": "em-ci-rust-result.v1",
        "repository": environment.get("EXPECTED_REPOSITORY", ""),
        "sha": environment.get("EXPECTED_SHA", ""),
        "run_id": environment.get("EXPECTED_RUN_ID", ""),
        "run_attempt": environment.get("EXPECTED_RUN_ATTEMPT", ""),
        "profile": "standard",
        "result": "success",
        "proof_outcome": "success",
    }
    if not re.fullmatch(r"[0-9a-f]{40}", expected["sha"]):
        raise ValueError("expected SHA is absent or invalid")
    if not re.fullmatch(r"[^/\s]+/[^/\s]+", expected["repository"]):
        raise ValueError("expected repository is absent or invalid")
    if any(not re.fullmatch(r"[1-9][0-9]*", expected[key]) for key in ("run_id", "run_attempt")):
        raise ValueError("expected run identity is absent or invalid")
    for key, value in expected.items():
        if evidence.get(key) != value:
            raise ValueError(f"missing or contradictory {key}; use Re-run all jobs for a fresh exact-attempt proof")
    route = evidence.get("route")
    reason = evidence.get("infrastructure_failure")
    if route == "hosted_fallback":
        if reason not in {"toolchain_unavailable", "disposable_paths_unavailable"}:
            raise ValueError("hosted recovery has no eligible typed admission withdrawal")
    elif route in {"self_hosted", "hosted"}:
        if reason != "":
            raise ValueError("successful primary proof contradicts admission withdrawal")
    else:
        raise ValueError("unknown selected route")
    entered = evidence.get("proof_entered_at", "")
    if not re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z", entered):
        raise ValueError("proof entry is missing or malformed")
    datetime.datetime.strptime(entered, "%Y-%m-%dT%H:%M:%SZ")
    return evidence


def main() -> int:
    """Emit validation only after the complete evidence contract passes."""
    try:
        evidence = validate_evidence(os.environ)
        output = os.environ.get("GITHUB_OUTPUT")
        if not output:
            raise ValueError("GitHub output path is absent")
        with Path(output).open("a", encoding="utf-8") as stream:
            stream.write("proof_validated=true\n")
        print(f"Rust Small proof validated: {evidence['route']}")
    except (ValueError, OSError) as error:
        print(f"::error::Rust Small NOT_PROVEN: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
