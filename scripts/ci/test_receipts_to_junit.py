#!/usr/bin/env python3
"""Focused tests for scripts/ci/receipts-to-junit.py."""

from __future__ import annotations

import importlib.util
import io
import json
import tempfile
import unittest
import xml.etree.ElementTree as ET
from contextlib import redirect_stdout
from pathlib import Path

SCRIPT = Path(__file__).with_name("receipts-to-junit.py")
SPEC = importlib.util.spec_from_file_location("receipts_to_junit", SCRIPT)
assert SPEC and SPEC.loader
receipts_to_junit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(receipts_to_junit)


def write(path: Path, payload: object) -> None:
    path.write_text(json.dumps(payload), encoding="utf-8")


def convert(path: Path, suite: str = "instrument"):
    return receipts_to_junit.receipts_to_junit(path, suite)


def atomic(*rows: dict) -> dict:
    return {"test_results_schema": 1, "test_results": list(rows)}


def display_name(row: dict) -> str:
    return receipts_to_junit.validate_atomic_entry(row).identity.display_name()


class ReceiptsToJunitTests(unittest.TestCase):
    def test_gate_receipt_is_not_emitted_as_tests(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "gate.json"
            write(
                receipt,
                {
                    "gates": [
                        {"gate_name": "fmt", "status": "pass"},
                        {
                            "gate_name": "unit_routed_full",
                            "status": "fail",
                            "command": "cargo test -p xtask",
                        },
                        {"gate_name": "clippy_full", "status": "timeout"},
                    ]
                },
            )
            root, total, failures, errors, skipped = convert(receipt, "pr-fast")
        self.assertEqual((0, 0, 0, 0), (total, failures, errors, skipped))
        self.assertEqual([], root.findall("./testsuite/testcase"))

    def test_different_gate_routes_cannot_collapse_to_one_identity(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            for index, command in enumerate(
                ("cargo test -p perl-lsp-rs", "cargo test -p xtask")
            ):
                write(
                    directory / f"route-{index}.json",
                    {
                        "gates": [
                            {
                                "gate_name": "unit_routed_full",
                                "status": "fail",
                                "command": command,
                            }
                        ]
                    },
                )
            root, total, failures, errors, skipped = convert(directory, "pr-fast")
        self.assertEqual((0, 0, 0, 0), (total, failures, errors, skipped))
        self.assertEqual([], root.findall("./testsuite/testcase"))

    def test_ux_aggregate_is_not_atomic(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "ux.json"
            write(
                receipt,
                {
                    "result": "fail",
                    "failure_class": "assertion",
                    "first_failing_test": "journey",
                },
            )
            root, total, failures, errors, skipped = convert(receipt, "ux")
        self.assertEqual((0, 0, 0, 0), (total, failures, errors, skipped))
        self.assertEqual([], root.findall("./testsuite/testcase"))

    def test_atomic_results_preserve_result_classes_and_locations(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "atomic.json"
            write(
                receipt,
                atomic(
                    {
                        "suite": "cap",
                        "name": "full",
                        "parameters": {"profile": "full", "client": "vscode"},
                        "status": "pass",
                        "duration_ms": 1250,
                    },
                    {
                        "suite": "cap",
                        "name": "commands",
                        "status": "fail",
                        "failure_message": "snapshot mismatch",
                        "file": "cap.rs",
                        "line": 171,
                    },
                    {
                        "suite": "parser",
                        "name": "recovery",
                        "status": "skip",
                        "message": "tracked",
                    },
                    {
                        "suite": "stdio",
                        "name": "initialize",
                        "status": "instrument_failure",
                        "message": "binary missing",
                    },
                ),
            )
            root, total, failures, errors, skipped = convert(receipt)
        self.assertEqual((4, 1, 1, 1), (total, failures, errors, skipped))
        names = {tc.get("name"): tc for tc in root.findall("./testsuite/testcase")}
        parameterized_name = (
            '@perl-lsp:junit:v1:params:4:full:{"client":"vscode","profile":"full"}'
        )
        self.assertIn(parameterized_name, names)
        self.assertEqual(
            "1.25",
            names[parameterized_name].get("time"),
        )
        self.assertIn("cap.rs:171", names["commands"].findtext("failure") or "")
        self.assertEqual(
            "instrument_failure",
            names["initialize"].find("error").get("type"),
        )

    def test_missing_input_is_explicit_skipped_instrumentation(self) -> None:
        root, total, failures, errors, skipped = convert(
            Path("missing-receipt-dir"), "missing"
        )
        self.assertEqual((1, 0, 0, 1), (total, failures, errors, skipped))
        node = root.find("./testsuite/testcase[@name='no-receipt-input']/skipped")
        self.assertIsNotNone(node)
        self.assertEqual("No receipt files found", node.get("message"))

    def test_invalid_and_unknown_json_emit_parse_errors(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            (directory / "invalid.json").write_text("{", encoding="utf-8")
            (directory / "unknown.json").write_text("[]", encoding="utf-8")
            root, total, failures, errors, skipped = convert(directory)
        self.assertEqual((0, 0, 2, 0), (total, failures, errors, skipped))
        self.assertEqual(
            [],
            root.findall("./testsuite/testcase"),
        )
        diagnostics = root.findtext("./testsuite/system-err") or ""
        self.assertIn("invalid.json: JSONDecodeError:", diagnostics)
        self.assertIn("unknown.json: UnrecognizedFormat:", diagnostics)

    def test_malformed_atomic_array_is_parse_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            write(
                directory / "scalar.json",
                {"test_results_schema": 1, "test_results": "broken"},
            )
            write(
                directory / "mixed.json",
                {
                    "test_results_schema": 1,
                    "test_results": [
                        {"suite": "suite", "name": "valid", "status": "pass"},
                        "not-an-object",
                    ],
                },
            )
            root, total, failures, errors, skipped = convert(directory)
        self.assertEqual((0, 0, 2, 0), (total, failures, errors, skipped))
        self.assertEqual([], root.findall("./testsuite/testcase"))

    def test_parameter_identity_is_injective_and_type_preserving(self) -> None:
        rows = [
            {
                "suite": "s",
                "name": "case",
                "parameters": {"case": 1},
                "status": "pass",
            },
            {
                "suite": "s",
                "name": "case",
                "parameters": {"case": "1"},
                "status": "pass",
            },
            {
                "suite": "s",
                "name": "case",
                "parameters": {"a": "x,b=y"},
                "status": "pass",
            },
            {
                "suite": "s",
                "name": "case",
                "parameters": {"a": "x", "b": "y"},
                "status": "pass",
            },
            {
                "suite": "s",
                "name": "case",
                "parameters": {},
                "status": "pass",
            },
            {"suite": "s", "name": "case", "status": "pass"},
        ]
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "parameters.json"
            write(receipt, atomic(*rows))
            root, total, failures, errors, skipped = convert(receipt)
        names = [tc.get("name") for tc in root.findall("./testsuite/testcase")]
        self.assertEqual((6, 0, 0, 0), (total, failures, errors, skipped))
        self.assertEqual(6, len(set(names)))

    def test_parameterized_and_json_like_unparameterized_names_are_distinct(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "suffix-collision.json"
            write(
                receipt,
                atomic(
                    {"suite": "s", "name": "case[{}]", "status": "fail"},
                    {
                        "suite": "s",
                        "name": "case",
                        "parameters": {},
                        "status": "pass",
                    },
                ),
            )
            root, total, failures, errors, skipped = convert(receipt)
        testcases = root.findall("./testsuite/testcase")
        self.assertEqual((2, 1, 0, 0), (total, failures, errors, skipped))
        self.assertEqual(2, len({testcase.get("name") for testcase in testcases}))
        self.assertEqual(
            {"case[{}]", "@perl-lsp:junit:v1:params:4:case:{}"},
            {testcase.get("name") for testcase in testcases},
        )

    def test_parameter_display_is_canonical_and_stable(self) -> None:
        first = {
            "suite": "s",
            "name": "case",
            "parameters": {
                "z": ["brackets[{}]", {"snowman": "☃"}],
                "a": {"emoji": "💡", "quote": 'x:"y"'},
            },
            "status": "pass",
        }
        reordered = {
            **first,
            "parameters": {
                "a": {"quote": 'x:"y"', "emoji": "💡"},
                "z": ["brackets[{}]", {"snowman": "☃"}],
            },
        }
        expected = display_name(first)
        self.assertEqual(expected, display_name(reordered))
        self.assertEqual(expected, display_name(first))
        self.assertIn("brackets[{}]", expected)
        self.assertIn("☃", expected)

    def test_empty_parameters_absent_parameters_and_reserved_names_are_distinct(self) -> None:
        rows = (
            {"suite": "s", "name": "case", "status": "pass"},
            {
                "suite": "s",
                "name": "case",
                "parameters": {},
                "status": "pass",
            },
            {
                "suite": "s",
                "name": "@perl-lsp:junit:v1:params:4:case:{}",
                "status": "pass",
            },
        )
        names = [display_name(row) for row in rows]
        self.assertEqual(3, len(set(names)))
        self.assertEqual("case", names[0])
        self.assertTrue(names[1].startswith("@perl-lsp:junit:v1:params:"))
        self.assertTrue(names[2].startswith("@perl-lsp:junit:v1:name:"))

    def test_duplicate_json_object_keys_are_parse_errors_recursively(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "duplicate-keys.json"
            receipt.write_text(
                '{"test_results_schema":1,"test_results":[{"suite":"s",'
                '"name":"case","parameters":{"profile":"full",'
                '"profile":"debug"},"status":"pass"}]}',
                encoding="utf-8",
            )
            root, total, failures, errors, skipped = convert(receipt)
        self.assertEqual((0, 0, 1, 0), (total, failures, errors, skipped))
        self.assertEqual([], root.findall("./testsuite/testcase"))
        self.assertIn(
            "duplicate JSON object key",
            root.findtext("./testsuite/system-err") or "",
        )

    def test_directory_atomic_rows_are_suppressed_when_any_receipt_is_invalid(self) -> None:
        cases = {
            "malformed": ("bad.json", "{"),
            "invalid": (
                "bad.json",
                json.dumps(
                    atomic({"suite": "s", "name": "case", "status": "unknown"})
                ),
            ),
            "duplicate": (
                "bad.json",
                json.dumps(
                    atomic(
                        {"suite": "s", "name": "case", "status": "pass"},
                        {"suite": "s", "name": "case", "status": "pass"},
                    )
                ),
            ),
        }
        for label, (bad_name, bad_contents) in cases.items():
            with self.subTest(label=label), tempfile.TemporaryDirectory() as tmp:
                directory = Path(tmp)
                write(
                    directory / "valid.json",
                    atomic({"suite": "s", "name": "valid", "status": "pass"}),
                )
                (directory / bad_name).write_text(bad_contents, encoding="utf-8")
                root, total, failures, errors, skipped = convert(directory)
                testcases = root.findall("./testsuite/testcase")
            self.assertEqual((0, 0, 1, 0), (total, failures, errors, skipped))
            self.assertEqual([], testcases)
            self.assertIn("bad.json:", root.findtext("./testsuite/system-err") or "")

    def test_atomic_rows_require_version_identity_parameters_and_closed_status(self) -> None:
        cases = {
            "version.json": {"test_results": []},
            "suite.json": atomic(
                {"suite": "", "name": "case", "status": "pass"}
            ),
            "name.json": atomic({"suite": "suite", "status": "pass"}),
            "status.json": atomic(
                {"suite": "suite", "name": "case", "status": "passed"}
            ),
            "parameters.json": atomic(
                {
                    "suite": "suite",
                    "name": "case",
                    "parameters": ["x"],
                    "status": "pass",
                }
            ),
        }
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            for filename, payload in cases.items():
                write(directory / filename, payload)
            root, total, failures, errors, skipped = convert(directory)
        self.assertEqual((0, 0, 5, 0), (total, failures, errors, skipped))

    def test_empty_atomic_population_is_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "empty.json"
            write(receipt, atomic())
            root, total, failures, errors, skipped = convert(receipt)
        self.assertEqual((0, 0, 1, 0), (total, failures, errors, skipped))
        self.assertIn(
            "must contain at least one atomic result",
            root.findtext("./testsuite/system-err") or "",
        )

    def test_identity_and_diagnostics_must_be_canonical_xml_safe_text(self) -> None:
        cases = {
            "leading-space.json": atomic(
                {"suite": " suite", "name": "case", "status": "pass"}
            ),
            "identity-control.json": atomic(
                {"suite": "suite", "name": "bad\u0001case", "status": "pass"}
            ),
            "diagnostic-control.json": atomic(
                {
                    "suite": "suite",
                    "name": "case",
                    "status": "fail",
                    "message": "bad\u0000message",
                }
            ),
            "file-space.json": atomic(
                {
                    "suite": "suite",
                    "name": "case",
                    "status": "fail",
                    "file": " cap.rs ",
                }
            ),
        }
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            for filename, payload in cases.items():
                write(directory / filename, payload)
            root, total, failures, errors, skipped = convert(directory)
            output = directory / "junit.xml"
            ET.ElementTree(root).write(output, encoding="utf-8", xml_declaration=True)
            reparsed = ET.parse(output)
        self.assertEqual((0, 0, 4, 0), (total, failures, errors, skipped))
        self.assertEqual([], reparsed.getroot().findall("./testsuite/testcase"))

    def test_duration_is_finite_nonnegative_and_bounded(self) -> None:
        cases = {
            "huge-int.json": atomic(
                {
                    "suite": "suite",
                    "name": "case",
                    "status": "pass",
                    "duration_ms": 10**400,
                }
            ),
            "over-cap.json": atomic(
                {
                    "suite": "suite",
                    "name": "case",
                    "status": "pass",
                    "duration_ms": receipts_to_junit.MAX_DURATION_MS + 1,
                }
            ),
            "negative.json": atomic(
                {
                    "suite": "suite",
                    "name": "case",
                    "status": "pass",
                    "duration_ms": -1,
                }
            ),
        }
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            for filename, payload in cases.items():
                write(directory / filename, payload)
            root, total, failures, errors, skipped = convert(directory)
        self.assertEqual((0, 0, 3, 0), (total, failures, errors, skipped))

    def test_atomic_row_schema_rejects_ignored_identity_dimensions(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "extra-row-field.json"
            write(
                receipt,
                atomic(
                    {
                        "suite": "suite",
                        "name": "case",
                        "status": "pass",
                        "platform": "windows",
                    }
                ),
            )
            root, total, failures, errors, skipped = convert(receipt)
        self.assertEqual((0, 0, 1, 0), (total, failures, errors, skipped))
        self.assertIn(
            "unknown fields: platform",
            root.findtext("./testsuite/system-err") or "",
        )

    def test_atomic_envelope_schema_rejects_ignored_identity_dimensions(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "extra-envelope-field.json"
            payload = atomic({"suite": "suite", "name": "case", "status": "pass"})
            payload["route"] = "windows"
            write(receipt, payload)
            root, total, failures, errors, skipped = convert(receipt)
        self.assertEqual((0, 0, 1, 0), (total, failures, errors, skipped))
        self.assertIn(
            "unknown fields: route",
            root.findtext("./testsuite/system-err") or "",
        )

    def test_duplicate_identity_in_one_file_is_error(self) -> None:
        row = {
            "suite": "s",
            "name": "case",
            "parameters": {"profile": "full"},
            "status": "pass",
        }
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "duplicate.json"
            write(receipt, atomic(row, row))
            root, total, failures, errors, skipped = convert(receipt)
        self.assertEqual((0, 0, 1, 0), (total, failures, errors, skipped))
        self.assertIn(
            "duplicate atomic identity",
            root.findtext("./testsuite/system-err") or "",
        )

    def test_duplicate_identity_across_files_is_error(self) -> None:
        row = {"suite": "s", "name": "case", "status": "pass"}
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            write(directory / "a.json", atomic(row))
            write(directory / "b.json", atomic(row))
            root, total, failures, errors, skipped = convert(directory)
        self.assertEqual((0, 0, 1, 0), (total, failures, errors, skipped))
        self.assertIn(
            "already emitted by",
            root.findtext("./testsuite/system-err") or "",
        )

    def test_main_returns_nonzero_on_converter_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            receipt, output = directory / "receipt.json", directory / "junit.xml"
            write(
                receipt,
                {"test_results_schema": 1, "test_results": "broken"},
            )
            old_argv = receipts_to_junit.sys.argv
            try:
                receipts_to_junit.sys.argv = [
                    "receipts-to-junit.py",
                    "--input",
                    str(receipt),
                    "--output",
                    str(output),
                    "--suite-name",
                    "instrument",
                ]
                with redirect_stdout(io.StringIO()):
                    status = receipts_to_junit.main()
            finally:
                receipts_to_junit.sys.argv = old_argv
        self.assertEqual(1, status)

    def test_main_writes_empty_suite_for_gate_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            receipt, output = directory / "receipt.json", directory / "junit.xml"
            write(
                receipt,
                {"gates": [{"gate_name": "fmt", "status": "pass"}]},
            )
            old_argv = receipts_to_junit.sys.argv
            try:
                receipts_to_junit.sys.argv = [
                    "receipts-to-junit.py",
                    "--input",
                    str(receipt),
                    "--output",
                    str(output),
                    "--suite-name",
                    "ci-gates",
                ]
                stdout = io.StringIO()
                with redirect_stdout(stdout):
                    status = receipts_to_junit.main()
            finally:
                receipts_to_junit.sys.argv = old_argv
            suite = ET.parse(output).getroot().find("testsuite")
        self.assertEqual(0, status)
        self.assertEqual("0", suite.get("tests"))
        self.assertIn("no atomic test identities emitted", stdout.getvalue())


if __name__ == "__main__":
    unittest.main()
