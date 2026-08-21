#!/usr/bin/env python3
"""Validate the repository-local Cargo.lock conflict-repair contract.

This is an offline source-contract validator.  It classifies only the curated
fixture anchors below; it is deliberately not a token grep and does not invoke
Cargo, Git, a network, or a lockfile helper.  A real helper seam does not exist
yet, so the fixture oracle is the adoption surface for depguard #22.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from tempfile import TemporaryDirectory


OUTCOMES = {
    "accepted_lock_preserved",
    "lock_conflict_requires_admission",
    "manifest_requires_lock_change",
    "branch_admission_preserved",
    "historical_text",
    "controlled_isolated_generation",
    "not_proven",
}

FIXTURE = Path("scripts/ci/fixtures/cargo_lock_conflict_policy.json")


class ValidationError(ValueError):
    """Raised when this validator cannot establish its bounded contract."""


def validate_semantics(
    source: str, line_number: int, semantics: dict[str, object]
) -> list[str]:
    """Require source meaning on the identified anchor statement."""
    lines = source.splitlines()
    if line_number < 1 or line_number > len(lines):
        return ["semantic anchor line is unavailable"]
    semantic_source = lines[line_number - 1]
    required = semantics.get("required", [])
    forbidden = semantics.get("forbidden", [])
    if not isinstance(required, list) or not isinstance(forbidden, list):
        return ["semantic required/forbidden assertions must be lists"]
    if not required and not forbidden:
        return ["semantic assertions must not both be empty"]
    errors = []
    for phrase in required:
        if not isinstance(phrase, str) or phrase not in semantic_source:
            errors.append(f"missing required source semantics: {phrase!r}")
    for phrase in forbidden:
        if not isinstance(phrase, str):
            errors.append(f"forbidden source semantics must be strings: {phrase!r}")
        elif phrase in semantic_source:
            errors.append(f"forbidden source semantics present: {phrase!r}")
    return errors


def classify(case: dict[str, object]) -> str:
    """Classify an explicitly scoped command-surface case."""
    context = case.get("context")
    command = case.get("command")
    if context == "conflict_repair" and command in {
        "cargo generate-lockfile",
        "cargo update",
        "delete-and-recreate-Cargo.lock",
    }:
        return "lock_conflict_requires_admission"
    if context == "conflict_repair" and command == "manifest-change":
        return "manifest_requires_lock_change"
    if context == "conflict_repair" and command == "accepted-lock":
        return "accepted_lock_preserved"
    if context == "conflict_repair" and command == "branch-admission":
        return "branch_admission_preserved"
    if context == "isolated_extracted_package" and command == "cargo generate-lockfile":
        return "controlled_isolated_generation"
    if context == "release_refresh" and command == "just bump-version":
        return "branch_admission_preserved"
    if context == "targeted_dependency" and command == "cargo update -p name":
        return "branch_admission_preserved"
    if context == "historical_archive" and command == "just bump-version":
        return "historical_text"
    return "not_proven"


def validate_transition(
    accepted_lock: bytes,
    proposed_lock: bytes | None,
    *,
    manifest_requires_lock: bool,
    temporary_lock_path: Path,
) -> str:
    """Classify a transition and prove the supplied temporary lock was untouched."""
    try:
        before = temporary_lock_path.read_bytes()
    except OSError:
        return "not_proven"
    if before != accepted_lock:
        return "not_proven"
    if manifest_requires_lock:
        result = "manifest_requires_lock_change"
    elif proposed_lock is None:
        result = "not_proven"
    elif proposed_lock == accepted_lock:
        result = "accepted_lock_preserved"
    else:
        result = "lock_conflict_requires_admission"
    try:
        unchanged = temporary_lock_path.read_bytes() == before
    except OSError:
        return "not_proven"
    return result if unchanged else "not_proven"


def load_fixture(root: Path) -> list[dict[str, object]]:
    try:
        data = json.loads((root / FIXTURE).read_text(encoding="utf-8"))
    except (FileNotFoundError, json.JSONDecodeError) as error:
        raise ValidationError(f"fixture is unavailable or invalid: {error}") from error
    cases = data.get("cases") if isinstance(data, dict) else None
    if not isinstance(cases, list) or not cases:
        raise ValidationError("fixture cases must be a non-empty list")
    return cases


def validate(root: Path) -> list[str]:
    cases = load_fixture(root)
    errors: list[str] = []
    seen: set[str] = set()
    for index, case in enumerate(cases):
        prefix = f"cases[{index}]"
        case_id = case.get("id")
        if not isinstance(case_id, str) or not case_id:
            errors.append(f"{prefix}.id must be non-empty")
            continue
        if case_id in seen:
            errors.append(f"duplicate case id: {case_id}")
        seen.add(case_id)
        path = case.get("path")
        line_number = case.get("line")
        needle = case.get("needle")
        expected = case.get("expected")
        if not isinstance(path, str) or not isinstance(line_number, int) or line_number < 1:
            errors.append(f"{prefix} must identify a positive source line")
        if not isinstance(needle, str) or not needle:
            errors.append(f"{prefix} must identify a non-empty source needle")
            continue
        if expected not in OUTCOMES:
            errors.append(f"{case_id}: unsupported expected outcome {expected!r}")
        source_path = root / path
        try:
            source = source_path.read_text(encoding="utf-8")
        except OSError as error:
            errors.append(f"{case_id}: source unavailable: {path}: {error}")
            continue
        if source.count(needle) != 1:
            errors.append(f"{case_id}: source needle must occur exactly once: {path}")
        lines = source.splitlines()
        if line_number > len(lines) or needle not in lines[line_number - 1]:
            errors.append(f"{case_id}: source line {line_number} does not contain its needle")
        actual = classify(case)
        if actual != expected:
            errors.append(f"{case_id}: classified {actual}, expected {expected}")
        semantics = case.get("semantics")
        if not isinstance(semantics, dict):
            errors.append(f"{case_id}: semantic assertions are required")
            continue
        for semantic_error in validate_semantics(
            source, line_number, semantics
        ):
            errors.append(f"{case_id}: {semantic_error}")

    expected_outcomes = OUTCOMES - {"not_proven"}
    missing = sorted(expected_outcomes - {case.get("expected") for case in cases})
    if missing:
        errors.append("fixture is missing outcomes: " + ", ".join(missing))
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    args = parser.parse_args()
    errors = validate(args.repo_root.resolve())
    if errors:
        for error in errors:
            print(f"FAIL: {error}")
        return 1
    with TemporaryDirectory() as temp:
        accepted = Path(temp) / "Cargo.lock"
        original = b"# accepted lock\n"
        accepted.write_bytes(original)
        if validate_transition(
            original,
            original,
            manifest_requires_lock=False,
            temporary_lock_path=accepted,
        ) != "accepted_lock_preserved":
            print("FAIL: compatible lock transition was not accepted")
            return 1
        if accepted.read_bytes() != original:
            print("FAIL: compatible transition mutated accepted lock")
            return 1
        if validate_transition(
            original,
            b"# different lock\n",
            manifest_requires_lock=True,
            temporary_lock_path=accepted,
        ) != "manifest_requires_lock_change":
            print("FAIL: manifest-required transition was not refused")
            return 1
        if accepted.read_bytes() != original:
            print("FAIL: manifest-required transition mutated accepted lock")
            return 1
    digest = hashlib.sha256((args.repo_root.resolve() / FIXTURE).read_bytes()).hexdigest()
    print(f"OK: cargo-lock-conflict-policy cases={len(load_fixture(args.repo_root.resolve()))} fixture_sha256={digest}")
    print("OK: accepted lock remains byte-identical; manifest-required change is refused")
    return 0


if __name__ == "__main__":
    sys.exit(main())
