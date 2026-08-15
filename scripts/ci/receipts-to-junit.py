#!/usr/bin/env python3
"""Convert stable atomic test receipts to JUnit for Codecov Test Analytics.

Repository gate receipts remain gate-health evidence in their JSON artifacts. They are
not tests: a gate name can cover different commands, package routes, or composite
contracts across runs. Treating those rows as JUnit testcases manufactures a false
longitudinal identity and causes deterministic failures to be labelled as flakes.

Only versioned receipts with a top-level ``test_results`` array are eligible for Test
Analytics. Legacy gate and UX aggregate receipts are recognised and deliberately
omitted, yielding an empty JUnit suite rather than synthetic pass/fail testcases.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
import unicodedata
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any, Mapping, NamedTuple, Optional


ATOMIC_TEST_RESULTS = "atomic_test_results"
AGGREGATE_TEST_GATE = "aggregate_test_gate"
DETERMINISTIC_NON_TEST_GATE = "deterministic_non_test_gate"
ATOMIC_SCHEMA_VERSION = 1
ATOMIC_ENVELOPE_FIELDS = frozenset({"test_results_schema", "test_results"})
ATOMIC_ROW_FIELDS = frozenset(
    {
        "suite",
        "name",
        "parameters",
        "status",
        "duration_ms",
        "message",
        "failure_message",
        "file",
        "line",
        "command",
    }
)
ATOMIC_STATUSES = {
    "pass",
    "fail",
    "skip",
    "not_applicable",
    "timeout",
    "cancelled",
    "instrument_failure",
    "not_proven",
}
MAX_IDENTITY_CHARS = 512
MAX_PARAMETERS_CHARS = 4096
MAX_MESSAGE_CHARS = 16384
MAX_FILE_CHARS = 4096
MAX_COMMAND_CHARS = 16384
MAX_DURATION_MS = 7 * 24 * 60 * 60 * 1000
_DISPLAY_NAME_PREFIX = "@perl-lsp:junit:v1:"


class ParsedReceipt(NamedTuple):
    """Classified receipt entries eligible—or deliberately ineligible—for JUnit."""

    kind: str
    entries: tuple[dict[str, Any], ...]


class AtomicIdentity(NamedTuple):
    """Validated, stable identity for one atomic test row."""

    suite: str
    name: str
    parameters: Optional[str]

    def display_name(self) -> str:
        """Encode the row identity injectively in JUnit's single name field.

        Ordinary unparameterized names keep their historical display. Names in the
        reserved namespace are escaped, while every parameterized identity uses a
        tagged, length-prefixed form. The tags distinguish absent parameters from an
        explicitly present empty object without relying on delimiters inside user text.
        """
        if self.parameters is None:
            if self.name.startswith(_DISPLAY_NAME_PREFIX):
                return (
                    f"{_DISPLAY_NAME_PREFIX}name:{len(self.name)}:{self.name}"
                )
            return self.name
        return (
            f"{_DISPLAY_NAME_PREFIX}params:{len(self.name)}:"
            f"{self.name}:{self.parameters}"
        )

    def key(self) -> tuple[str, str, Optional[str]]:
        return self.suite, self.name, self.parameters


class ValidatedAtomic(NamedTuple):
    entry: Mapping[str, Any]
    identity: AtomicIdentity


def _mapping_entries(
    value: Any, *, field: str, require_nonempty: bool = False
) -> Optional[tuple[dict[str, Any], ...]]:
    if not isinstance(value, list) or any(not isinstance(entry, dict) for entry in value):
        return None
    if require_nonempty and not value:
        raise ValueError(f"{field} must contain at least one atomic result")
    return tuple(value)


def parse_receipt(data: Any) -> Optional[ParsedReceipt]:
    """Classify one receipt without inventing atomic test identity.

    ``test_results`` is the only accepted atomic surface and is versioned so future
    shapes cannot silently inherit this identity contract. Gate receipts and UX
    regression summaries are recognised so callers can distinguish "known aggregate,
    intentionally omitted" from malformed or unknown input.
    """
    if not isinstance(data, dict):
        return None

    if "test_results" in data or "test_results_schema" in data:
        unknown = set(data) - ATOMIC_ENVELOPE_FIELDS
        if unknown:
            raise ValueError(
                "atomic result envelope has unknown fields: " + ", ".join(sorted(unknown))
            )
        if data.get("test_results_schema") != ATOMIC_SCHEMA_VERSION:
            return None
        entries = _mapping_entries(
            data.get("test_results"),
            field="test_results",
            require_nonempty=True,
        )
        if entries is None:
            return None
        return ParsedReceipt(ATOMIC_TEST_RESULTS, entries)

    if "gates" in data:
        entries = _mapping_entries(data["gates"], field="gates")
        if entries is None:
            return None
        kind = (
            DETERMINISTIC_NON_TEST_GATE
            if entries and all(_is_deterministic_non_test_gate(entry) for entry in entries)
            else AGGREGATE_TEST_GATE
        )
        return ParsedReceipt(kind, entries)

    if "result" in data or "failure_class" in data:
        return ParsedReceipt(AGGREGATE_TEST_GATE, (data,))

    return None


def _is_deterministic_non_test_gate(entry: Mapping[str, Any]) -> bool:
    """Recognise common non-test gates without making the list authoritative."""
    name = str(entry.get("gate_name") or entry.get("name") or "")
    tags = {str(tag) for tag in entry.get("tags", []) if isinstance(tag, str)}
    return name.startswith(("fmt", "clippy", "check_")) or bool(
        tags.intersection({"formatting", "lint", "compile", "hygiene"})
    )


def _is_xml_char(character: str) -> bool:
    codepoint = ord(character)
    return (
        codepoint in (0x9, 0xA, 0xD)
        or 0x20 <= codepoint <= 0xD7FF
        or 0xE000 <= codepoint <= 0xFFFD
        or 0x10000 <= codepoint <= 0x10FFFF
    )


def _identity_text(entry: Mapping[str, Any], field: str) -> str:
    value = entry.get(field)
    if not isinstance(value, str) or not value:
        raise ValueError(f"atomic result requires non-empty string {field!r}")
    if value != value.strip():
        raise ValueError(f"atomic result {field!r} must not have surrounding whitespace")
    if len(value) > MAX_IDENTITY_CHARS:
        raise ValueError(
            f"atomic result {field!r} exceeds {MAX_IDENTITY_CHARS} characters"
        )
    if any(not _is_xml_char(character) for character in value):
        raise ValueError(f"atomic result {field!r} contains an XML-illegal character")
    if any(unicodedata.category(character).startswith("C") for character in value):
        raise ValueError(f"atomic result {field!r} must contain printable text only")
    return value


def _optional_text(
    entry: Mapping[str, Any],
    field: str,
    *,
    max_chars: int,
    canonical: bool = False,
) -> Optional[str]:
    if field not in entry:
        return None
    value = entry[field]
    if not isinstance(value, str) or not value:
        raise ValueError(f"atomic result {field!r} must be a non-empty string when present")
    if canonical and value != value.strip():
        raise ValueError(f"atomic result {field!r} must not have surrounding whitespace")
    if len(value) > max_chars:
        raise ValueError(f"atomic result {field!r} exceeds {max_chars} characters")
    if any(not _is_xml_char(character) for character in value):
        raise ValueError(f"atomic result {field!r} contains an XML-illegal character")
    return value


def _safe_error_text(value: object, max_chars: int = MAX_MESSAGE_CHARS) -> str:
    """Make converter-generated diagnostics valid XML without hiding input failure."""
    text = str(value)
    text = "".join(character if _is_xml_char(character) else "\uFFFD" for character in text)
    if len(text) > max_chars:
        text = text[: max_chars - 1] + "…"
    return text


def _reject_duplicate_json_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON object key: {key!r}")
        result[key] = value
    return result


def _canonical_parameters(entry: Mapping[str, Any]) -> Optional[str]:
    if "parameters" not in entry:
        return None
    parameters = entry["parameters"]
    if not isinstance(parameters, dict):
        raise ValueError("atomic result parameters must be a JSON object")
    try:
        encoded = json.dumps(
            parameters,
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=False,
            allow_nan=False,
        )
    except (TypeError, ValueError) as error:
        raise ValueError(f"atomic result parameters are not canonical JSON: {error}") from error
    if len(encoded) > MAX_PARAMETERS_CHARS:
        raise ValueError(
            f"atomic result parameters exceed {MAX_PARAMETERS_CHARS} canonical characters"
        )
    if any(not _is_xml_char(character) for character in encoded):
        raise ValueError("atomic result parameters contain an XML-illegal character")
    return encoded


def _duration_ms(entry: Mapping[str, Any]) -> float:
    value = entry.get("duration_ms", 0)
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError("atomic result duration_ms must be a bounded non-negative number")
    if isinstance(value, int):
        if value < 0 or value > MAX_DURATION_MS:
            raise ValueError(
                f"atomic result duration_ms must be between 0 and {MAX_DURATION_MS}"
            )
        return float(value)
    if not math.isfinite(value) or value < 0 or value > MAX_DURATION_MS:
        raise ValueError(
            f"atomic result duration_ms must be finite and between 0 and {MAX_DURATION_MS}"
        )
    return value


def validate_atomic_entry(entry: Mapping[str, Any]) -> ValidatedAtomic:
    unknown = set(entry) - ATOMIC_ROW_FIELDS
    if unknown:
        raise ValueError(
            "atomic result row has unknown fields: " + ", ".join(sorted(unknown))
        )

    suite = _identity_text(entry, "suite")
    name = _identity_text(entry, "name")
    status = entry.get("status")
    if not isinstance(status, str) or status not in ATOMIC_STATUSES:
        raise ValueError(
            "atomic result status must be one of " + ", ".join(sorted(ATOMIC_STATUSES))
        )

    normalized: dict[str, Any] = {
        "suite": suite,
        "name": name,
        "status": status,
        "duration_ms": _duration_ms(entry),
    }
    parameters = _canonical_parameters(entry)
    if parameters is not None:
        normalized["parameters"] = parameters

    for field, max_chars, canonical in (
        ("message", MAX_MESSAGE_CHARS, False),
        ("failure_message", MAX_MESSAGE_CHARS, False),
        ("file", MAX_FILE_CHARS, True),
        ("command", MAX_COMMAND_CHARS, True),
    ):
        value = _optional_text(
            entry,
            field,
            max_chars=max_chars,
            canonical=canonical,
        )
        if value is not None:
            normalized[field] = value

    if "line" in entry:
        line = entry["line"]
        if isinstance(line, bool) or not isinstance(line, int) or not 1 <= line <= 2_147_483_647:
            raise ValueError("atomic result line must be a positive 32-bit integer")
        normalized["line"] = line

    identity = AtomicIdentity(suite, name, parameters)
    return ValidatedAtomic(normalized, identity)


def atomic_to_testcase(validated: ValidatedAtomic) -> ET.Element:
    """Convert one validated stable atomic result into a JUnit testcase."""
    entry = validated.entry
    identity = validated.identity
    duration_sec = float(entry["duration_ms"]) / 1000.0
    status = str(entry["status"])

    testcase = ET.Element("testcase")
    testcase.set("classname", identity.suite)
    testcase.set("name", identity.display_name())
    testcase.set("time", str(duration_sec))

    if status == "pass":
        return testcase

    if status in ("skip", "not_applicable"):
        skipped = ET.SubElement(testcase, "skipped")
        skipped.set("message", str(entry.get("message") or "Skipped"))
        return testcase

    is_error = status in (
        "timeout",
        "cancelled",
        "instrument_failure",
        "not_proven",
    )
    tag = "error" if is_error else "failure"
    result = ET.SubElement(testcase, tag)
    result.set("type", status)

    diagnostic = []
    message = entry.get("message") or entry.get("failure_message")
    if message:
        diagnostic.append(str(message))
    file_path = entry.get("file")
    line = entry.get("line")
    if file_path:
        location = str(file_path)
        if line is not None:
            location = f"{location}:{line}"
        diagnostic.append(f"Location: {location}")
    command = entry.get("command")
    if command:
        diagnostic.append(f"Command: {command}")
    result.text = "\n".join(diagnostic)
    return testcase


def _validate_atomic_file(
    entries: tuple[dict[str, Any], ...],
    json_file: Path,
    globally_seen: Mapping[tuple[str, str, Optional[str]], Path],
) -> list[ValidatedAtomic]:
    validated: list[ValidatedAtomic] = []
    locally_seen: set[tuple[str, str, Optional[str]]] = set()
    for index, entry in enumerate(entries):
        try:
            item = validate_atomic_entry(entry)
        except ValueError as error:
            raise ValueError(f"test_results[{index}]: {error}") from error
        key = item.identity.key()
        if key in locally_seen:
            raise ValueError(
                "duplicate atomic identity "
                f"{item.identity.suite}::{item.identity.display_name()} in {json_file}"
            )
        if key in globally_seen:
            raise ValueError(
                "duplicate atomic identity "
                f"{item.identity.suite}::{item.identity.display_name()} "
                f"already emitted by {globally_seen[key]}"
            )
        locally_seen.add(key)
        validated.append(item)
    return validated


def _empty_input_suite(suite_name: str, message: str) -> tuple[ET.Element, int, int, int, int]:
    root = ET.Element("testsuites")
    testsuite = ET.SubElement(root, "testsuite")
    testsuite.set("name", suite_name)
    testsuite.set("tests", "1")
    testsuite.set("failures", "0")
    testsuite.set("skipped", "1")
    testsuite.set("errors", "0")
    testsuite.set("time", "0.0")

    testcase = ET.SubElement(testsuite, "testcase")
    testcase.set("classname", suite_name)
    testcase.set("name", "no-receipt-input")
    testcase.set("time", "0.0")
    skipped = ET.SubElement(testcase, "skipped")
    skipped.set("message", message)
    return root, 1, 0, 0, 1


def _json_files(input_path: Path) -> Optional[list[Path]]:
    if not input_path.exists():
        return None
    if input_path.is_file():
        return [input_path]
    if input_path.is_dir():
        return sorted(input_path.glob("*.json"))
    return []


def receipts_to_junit(
    input_path: Path, suite_name: str
) -> tuple[ET.Element, int, int, int, int]:
    """Convert only stable atomic test results from receipt file(s)."""
    json_files = _json_files(input_path)
    if json_files is None:
        return _empty_input_suite(suite_name, "No receipt files found")
    if not json_files:
        return _empty_input_suite(suite_name, "No JSON files found")

    root = ET.Element("testsuites")
    all_testcases: list[ET.Element] = []
    atomic_testcases: list[ET.Element] = []
    atomic_time = 0.0
    globally_seen: dict[tuple[str, str, Optional[str]], Path] = {}
    input_errors: list[str] = []

    for json_file in json_files:
        try:
            data = json.loads(
                json_file.read_text(encoding="utf-8"),
                object_pairs_hook=_reject_duplicate_json_keys,
            )
            parsed = parse_receipt(data)
            if parsed is None:
                input_errors.append(
                    _parse_error_text(json_file, "UnrecognizedFormat")
                )
                continue

            if parsed.kind != ATOMIC_TEST_RESULTS:
                # Gate-health and aggregate summaries stay in their JSON receipt artifacts.
                # An empty JUnit suite is the honest Test Analytics projection.
                continue

            validated = _validate_atomic_file(parsed.entries, json_file, globally_seen)
            for item in validated:
                testcase = atomic_to_testcase(item)
                atomic_time += float(testcase.get("time", 0.0))
                atomic_testcases.append(testcase)
            for item in validated:
                globally_seen[item.identity.key()] = json_file
        except json.JSONDecodeError as error:
            input_errors.append(
                _parse_error_text(
                    json_file,
                    "JSONDecodeError",
                    str(error),
                )
            )
        except (OSError, OverflowError, TypeError, ValueError) as error:
            input_errors.append(
                _parse_error_text(
                    json_file,
                    type(error).__name__,
                    str(error),
                )
            )

    if not input_errors:
        all_testcases = atomic_testcases + all_testcases
    total_time = atomic_time if not input_errors else 0.0

    testsuite = ET.SubElement(root, "testsuite")
    testsuite.set("name", suite_name)
    total_tests = len(all_testcases)
    total_failures = sum(1 for testcase in all_testcases if testcase.find("failure") is not None)
    total_errors = sum(1 for testcase in all_testcases if testcase.find("error") is not None)
    total_skipped = sum(1 for testcase in all_testcases if testcase.find("skipped") is not None)
    testsuite.set("tests", str(total_tests))
    testsuite.set("failures", str(total_failures))
    testsuite.set("errors", str(total_errors))
    testsuite.set("skipped", str(total_skipped))
    testsuite.set("time", str(total_time))
    for testcase in all_testcases:
        testsuite.append(testcase)
    if input_errors:
        system_error = ET.SubElement(testsuite, "system-err")
        system_error.text = "\n".join(input_errors)

    # Input errors are not test results and therefore never become JUnit testcases.
    # The separate return count keeps the CLI exit nonzero without presenting a
    # filename-derived longitudinal test identity to Codecov.
    return root, total_tests, total_failures, total_errors + len(input_errors), total_skipped


def _parse_error_text(
    json_file: Path,
    error_type: str,
    detail: Optional[str] = None,
) -> str:
    message = _safe_error_text(
        detail or f"Receipt at {json_file} does not match a known format"
    )
    filename = _safe_error_text(json_file.name, MAX_FILE_CHARS)
    safe_type = _safe_error_text(error_type, MAX_IDENTITY_CHARS)
    return f"{filename}: {safe_type}: {message}"


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Convert stable atomic test receipts to JUnit for Codecov Test Analytics."
    )
    parser.add_argument(
        "--input",
        required=True,
        type=Path,
        help="Input: one JSON receipt or a directory containing receipt JSON files",
    )
    parser.add_argument(
        "--output",
        required=True,
        type=Path,
        help="Output JUnit XML file path",
    )
    parser.add_argument(
        "--suite-name",
        required=True,
        help="Instrumentation suite name for converter errors and aggregate projections",
    )
    args = parser.parse_args()

    args.output.parent.mkdir(parents=True, exist_ok=True)
    root, total_tests, total_failures, total_errors, total_skipped = receipts_to_junit(
        args.input, args.suite_name
    )
    ET.ElementTree(root).write(args.output, encoding="utf-8", xml_declaration=True)

    print(f"Generated JUnit XML: {args.output}")
    print(f"  Suite: {args.suite_name}")
    print(
        "  Atomic tests: "
        f"{total_tests}, Failures: {total_failures}, Errors: {total_errors}, "
        f"Skipped: {total_skipped}"
    )
    if total_tests == 0:
        print("  Aggregate gate receipt recognised; no atomic test identities emitted")
    if total_errors:
        print("  Converter/instrument errors detected; Test Analytics upload is not proven")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
