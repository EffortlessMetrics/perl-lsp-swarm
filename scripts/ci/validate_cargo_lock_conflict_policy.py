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
import re
import sys
from collections.abc import Mapping
from pathlib import Path, PurePosixPath, PureWindowsPath
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
INVENTORY_ROOTS = (Path(".ci"), Path(".github/workflows"), Path("docs"), Path("scripts/ci"))
INVENTORY_EXCLUSIONS = {
    FIXTURE.as_posix(),
    "scripts/ci/test_validate_cargo_lock_conflict_policy.py",
    "scripts/ci/validate_cargo_lock_conflict_policy.py",
}
INVENTORY_ARCHIVE_PREFIXES = ("docs/reference/archive/",)
INVENTORY_TEXT_SUFFIXES = {".json", ".md", ".py", ".sh", ".toml", ".yaml", ".yml"}
LOCK_COMMAND_RE = re.compile(
    r"(?P<cargo>\bcargo\s+(?:generate-lockfile\b|update\b))|"
    r"(?P<delete>\bdelete\s*/\s*recreate\s+`?Cargo\.lock`?)",
    re.IGNORECASE,
)


class ValidationError(ValueError):
    """Raised when this validator cannot establish its bounded contract."""


def validate_semantics(
    source: str, line_number: int, semantics: dict[str, object]
) -> list[str]:
    """Require source meaning on the identified line or Markdown section."""
    lines = source.splitlines()
    scope = semantics.get("scope", "line")
    semantic_range, range_error = semantic_line_range(source, line_number, scope)
    if semantic_range is None:
        return [range_error]
    start, end = semantic_range
    semantic_source = "\n".join(lines[start - 1 : end])
    required = semantics.get("required", [])
    forbidden = semantics.get("forbidden", [])
    forbidden_commands = semantics.get("forbidden_commands", [])
    if (
        not isinstance(required, list)
        or not isinstance(forbidden, list)
        or not isinstance(forbidden_commands, list)
    ):
        return [
            "semantic required, forbidden, and forbidden_commands assertions "
            "must be lists"
        ]
    if not required and not forbidden:
        if not forbidden_commands:
            return ["semantic assertions must not both be empty"]
    errors = []
    for phrase in required:
        if not isinstance(phrase, str):
            errors.append(f"required source semantics must be strings: {phrase!r}")
        elif not phrase.strip():
            errors.append("required source semantics must not be empty")
        elif phrase not in semantic_source:
            errors.append(f"missing required source semantics: {phrase!r}")
    for phrase in forbidden:
        if not isinstance(phrase, str):
            errors.append(f"forbidden source semantics must be strings: {phrase!r}")
        elif not phrase.strip():
            errors.append("forbidden source semantics must not be empty")
        elif phrase in semantic_source:
            errors.append(f"forbidden source semantics present: {phrase!r}")
    for command in forbidden_commands:
        if not isinstance(command, str):
            errors.append(f"forbidden source commands must be strings: {command!r}")
            continue
        if not command.strip():
            errors.append("forbidden source commands must not be empty")
            continue
        command_error = False
        for source_line in semantic_source.splitlines():
            lowered_line = source_line.lower()
            search_start = 0
            while True:
                occurrence = lowered_line.find(command.lower(), search_start)
                if occurrence < 0:
                    break
                prefix = lowered_line[:occurrence]
                refusal_span = re.split(r"[.;:\u2013\u2014]", prefix)[-1]
                comma_tail = prefix.rsplit(",", 1)[-1]
                if "," in prefix and re.search(
                    r"\b(?:then|but|however|unless|run|use|try)\b", comma_tail
                ):
                    refusal_span = comma_tail
                denial_pattern = (
                    r"\b(?:must\s+not|do\s+not|don't|never|not\s+authorize|"
                    r"refuse|prohibited|forbidden)\b"
                )
                suffix = lowered_line[occurrence + len(command) :]
                suffix_clause = re.split(r"[.;:,\u2013\u2014]", suffix)[0]
                contradiction_pattern = r"\b(?:allowed|permitted|run|use|try)\b"
                if (
                    re.search(denial_pattern, refusal_span) is None
                    or re.search(contradiction_pattern, suffix_clause) is not None
                ):
                    errors.append(
                        f"forbidden command lacks an explicit refusal: {command!r}"
                    )
                    command_error = True
                    break
                search_start = occurrence + len(command)
            if command_error:
                break
    return errors


def semantic_line_range(
    source: str, line_number: int, scope: object
) -> tuple[tuple[int, int] | None, str]:
    """Return the one-based source range covered by one fixture case."""
    lines = source.splitlines()
    if not isinstance(line_number, int) or line_number < 1 or line_number > len(lines):
        return None, "semantic anchor line is unavailable"
    if scope == "line":
        return (line_number, line_number), ""
    if scope != "section":
        return None, "semantic scope must be 'line' or 'section'"
    heading = re.match(r"^(#+)\s+", lines[line_number - 1])
    if heading is None:
        return None, "section semantic scope requires a Markdown heading anchor"
    level = len(heading.group(1))
    end = len(lines)
    fence_character: str | None = None
    for index in range(line_number, len(lines)):
        fence = re.match(r"^\s*(`{3,}|~{3,})", lines[index])
        if fence is not None:
            character = fence.group(1)[0]
            if fence_character is None:
                fence_character = character
            elif character == fence_character:
                fence_character = None
            continue
        if fence_character is not None:
            continue
        next_heading = re.match(r"^(#+)\s+", lines[index])
        if next_heading is not None and len(next_heading.group(1)) <= level:
            end = index
            break
    return (line_number, end), ""


def classify(case: Mapping[str, object]) -> str:
    """Classify an explicitly scoped command-surface case."""
    context = case.get("context")
    command = case.get("command")
    if not isinstance(context, str) or not isinstance(command, (str, type(None))):
        return "not_proven"
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
    if context == "targeted_dependency" and (
        command == "cargo update -p name" or command.startswith("cargo update -p ")
    ):
        return "branch_admission_preserved"
    if context == "release_refresh" and command in {
        "cargo update",
        "cargo update --workspace",
    }:
        return "branch_admission_preserved"
    if context == "dependency_maintenance" and command == "cargo update":
        return "branch_admission_preserved"
    if context == "historical_archive" and command == "cargo update":
        return "historical_text"
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


def load_fixture(root: Path) -> list[object]:
    try:
        data = json.loads((root / FIXTURE).read_text(encoding="utf-8"))
    except (
        FileNotFoundError,
        json.JSONDecodeError,
        OSError,
        UnicodeDecodeError,
    ) as error:
        raise ValidationError("fixture is unavailable or invalid") from error
    if not isinstance(data, dict):
        raise ValidationError("fixture must be a mapping")
    schema_version = data.get("schema_version")
    if type(schema_version) is not int or schema_version != 1:
        raise ValidationError("fixture schema_version must be exactly 1")
    claim_boundary = data.get("claim_boundary")
    if not isinstance(claim_boundary, str) or not claim_boundary.strip():
        raise ValidationError("fixture claim_boundary must be non-empty")
    cases = data.get("cases")
    if not isinstance(cases, list) or not cases:
        raise ValidationError("fixture cases must be a non-empty list")
    return cases


def discover_command_occurrences(root: Path) -> dict[str, set[int]]:
    """Find lock-repair command lines in tracked-style source files."""
    occurrences: dict[str, set[int]] = {}
    for relative_root in INVENTORY_ROOTS:
        source_root = root / relative_root
        if not source_root.is_dir():
            continue
        for path in source_root.rglob("*"):
            if not path.is_file():
                continue
            if "__pycache__" in path.parts:
                continue
            if path.name != "justfile" and path.suffix.lower() not in INVENTORY_TEXT_SUFFIXES:
                continue
            relative = path.relative_to(root).as_posix()
            if relative in INVENTORY_EXCLUSIONS:
                continue
            if relative.startswith(INVENTORY_ARCHIVE_PREFIXES):
                continue
            try:
                source = path.read_text(encoding="utf-8")
            except (OSError, UnicodeDecodeError) as error:
                raise ValidationError(
                    f"command-surface inventory could not read {relative}: {error}"
                ) from error
            lines = {
                line_number
                for line_number, line in enumerate(source.splitlines(), start=1)
                if LOCK_COMMAND_RE.search(line)
            }
            if lines:
                occurrences[relative] = lines
    return occurrences


def discover_command_surfaces(root: Path) -> set[str]:
    """Find tracked-style source files containing lock-repair commands."""
    return set(discover_command_occurrences(root))


def needle_occurrences(source: str, needle: str) -> int:
    """Count source needles, including overlapping occurrences."""
    return sum(
        source.startswith(needle, offset)
        for offset in range(len(source) - len(needle) + 1)
    )


def anchor_line(source: str, needle: str) -> int | None:
    """Return the one-based line containing the unique source needle."""
    if needle_occurrences(source, needle) != 1:
        return None
    offset = source.index(needle)
    line_number = source.count("\n", 0, offset) + 1
    line = source.splitlines()[line_number - 1]
    return line_number if needle in line else None


def validate(root: Path) -> list[str]:
    cases = load_fixture(root)
    errors: list[str] = []
    seen: set[str] = set()
    covered_paths: set[str] = set()
    covered_ranges: dict[str, list[tuple[int, int]]] = {}
    for index, case in enumerate(cases):
        prefix = f"cases[{index}]"
        if not isinstance(case, Mapping):
            errors.append(f"{prefix} must be a mapping")
            continue
        case_id = case.get("id")
        if not isinstance(case_id, str) or not case_id:
            errors.append(f"{prefix}.id must be non-empty")
            continue
        if case_id in seen:
            errors.append(f"duplicate case id: {case_id}")
        seen.add(case_id)
        path = case.get("path")
        needle = case.get("needle")
        expected = case.get("expected")
        if not isinstance(path, str):
            errors.append(f"{prefix}.path must be a string")
            continue
        covered_paths.add(path.replace("\\", "/"))
        if not isinstance(needle, str) or not needle:
            errors.append(f"{prefix} must identify a non-empty source needle")
            continue
        if not isinstance(expected, str) or expected not in OUTCOMES:
            errors.append(f"{case_id}: unsupported expected outcome {expected!r}")
        context = case.get("context")
        command = case.get("command")
        if not isinstance(context, str):
            errors.append(f"{case_id}: context must be a string")
        if not isinstance(command, (str, type(None))):
            errors.append(f"{case_id}: command must be a string or null")
        relative_path = Path(path)
        windows_path = PureWindowsPath(path)
        if (
            relative_path.is_absolute()
            or PurePosixPath(path).is_absolute()
            or windows_path.is_absolute()
            or bool(windows_path.drive or windows_path.root)
        ):
            errors.append(f"{case_id}: source path must be relative: {path}")
            continue
        normalized_parts = path.replace("\\", "/").split("/")
        if ".." in normalized_parts:
            errors.append(f"{case_id}: source path escapes repository root: {path}")
            continue
        try:
            source_path = (root / relative_path).resolve()
        except (OSError, ValueError) as error:
            errors.append(f"{case_id}: source unavailable: {path}: {error}")
            continue
        try:
            source_path.relative_to(root.resolve())
        except ValueError:
            errors.append(f"{case_id}: source path escapes repository root: {path}")
            continue
        try:
            source = source_path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError, ValueError) as error:
            errors.append(f"{case_id}: source unavailable: {path}: {error}")
            continue
        line_number = anchor_line(source, needle)
        if needle_occurrences(source, needle) != 1:
            errors.append(f"{case_id}: source needle must occur exactly once: {path}")
        elif line_number is None:
            errors.append(f"{case_id}: source needle must occur on one line: {path}")
        actual = classify(case)
        if actual != expected:
            errors.append(f"{case_id}: classified {actual}, expected {expected}")
        semantics = case.get("semantics")
        if not isinstance(semantics, dict):
            errors.append(f"{case_id}: semantic assertions are required")
            continue
        if line_number is not None:
            semantic_range, _ = semantic_line_range(
                source, line_number, semantics.get("scope", "line")
            )
            if semantic_range is not None:
                covered_ranges.setdefault(path.replace("\\", "/"), []).append(
                    semantic_range
                )
            for semantic_error in validate_semantics(source, line_number, semantics):
                errors.append(f"{case_id}: {semantic_error}")

    expected_outcomes = OUTCOMES - {"not_proven"}
    observed = {
        case.get("expected")
        for case in cases
        if isinstance(case, Mapping) and isinstance(case.get("expected"), str)
    }
    missing = sorted(expected_outcomes - observed)
    if missing:
        errors.append("fixture is missing outcomes: " + ", ".join(missing))
    discovered_paths = discover_command_surfaces(root)
    unregistered = sorted(discovered_paths - covered_paths)
    if unregistered:
        errors.append(
            "fixture is missing command-surface cases: " + ", ".join(unregistered)
        )
    for path, lines in discover_command_occurrences(root).items():
        ranges = covered_ranges.get(path, [])
        uncovered = sorted(
            line for line in lines if not any(start <= line <= end for start, end in ranges)
        )
        if uncovered:
            errors.append(
                f"fixture is missing command anchors: {path}:"
                + ",".join(str(line) for line in uncovered)
            )
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    args = parser.parse_args()
    try:
        errors = validate(args.repo_root.resolve())
    except ValidationError as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 2
    if errors:
        for error in errors:
            print(f"FAIL: {error}")
        return 1
    with TemporaryDirectory() as temp:
        accepted = Path(temp) / "Cargo.lock"
        original = b"# accepted lock\n"
        accepted.write_bytes(original)
        if (
            validate_transition(
                original,
                original,
                manifest_requires_lock=False,
                temporary_lock_path=accepted,
            )
            != "accepted_lock_preserved"
        ):
            print("FAIL: compatible lock transition was not accepted")
            return 1
        if accepted.read_bytes() != original:
            print("FAIL: compatible transition mutated accepted lock")
            return 1
        if (
            validate_transition(
                original,
                b"# different lock\n",
                manifest_requires_lock=True,
                temporary_lock_path=accepted,
            )
            != "manifest_requires_lock_change"
        ):
            print("FAIL: manifest-required transition was not refused")
            return 1
        if accepted.read_bytes() != original:
            print("FAIL: manifest-required transition mutated accepted lock")
            return 1
    digest = hashlib.sha256(
        (args.repo_root.resolve() / FIXTURE).read_bytes()
    ).hexdigest()
    print(
        f"OK: cargo-lock-conflict-policy cases={len(load_fixture(args.repo_root.resolve()))} fixture_sha256={digest}"
    )
    print(
        "OK: accepted lock remains byte-identical; manifest-required change is refused"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
