#!/usr/bin/env python3
"""Fixture tests for extract-failed-tests.py's typed verdict contract (#15349).

Run directly (no third-party deps -- stdlib unittest only):

    python3 .ci/scripts/test_extract_failed_tests.py

These tests exist because the previous extractor silently skipped every
non-JSON line and exited 0 even when it parsed no valid test events, so a
broken harness (compile failure, format drift, truncation, unreadable
input) collapsed into an empty failed-tests.txt and a green "no flakes"
scheduled run. They pin the typed fail-closed verdicts and the exact
(package, target, test-name) failure identity required before any replay
or quarantine mutation.
"""

import contextlib
import importlib.util
import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

_MODULE_PATH = Path(__file__).parent / "extract-failed-tests.py"
_SPEC = importlib.util.spec_from_file_location("extract_failed_tests", _MODULE_PATH)
_MODULE = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_MODULE)


def envelope(pkg, target="lib"):
    return json.dumps(
        {
            "type": "run",
            "package": pkg,
            "target": target,
            "command": ["cargo", "test", "-p", pkg, "--lib", "--locked"],
        }
    )


def run_end(pkg, status):
    return json.dumps({"type": "run_end", "package": pkg, "exit_status": status})


def suite_start(n):
    return json.dumps({"type": "suite", "event": "started", "test_count": n})


def suite_end(event, passed, failed, ignored=0):
    return json.dumps(
        {
            "type": "suite",
            "event": event,
            "passed": passed,
            "failed": failed,
            "ignored": ignored,
        }
    )


def test_event(event, name):
    return json.dumps({"type": "test", "event": event, "name": name})


class ExtractFailedTestsTest(unittest.TestCase):
    def parse(self, lines):
        return _MODULE.parse_observation_stream(lines)

    def test_compile_failure_before_any_test_event_is_command_failed(self):
        lines = [
            envelope("pkg_a"),
            "error: could not compile `pkg_a` (lib) due to 3 previous errors",
            run_end("pkg_a", 101),
        ]
        receipt = self.parse(lines)
        self.assertEqual(receipt["verdict"], "COMMAND_FAILED")
        self.assertEqual(receipt["exit_code"], 2)
        self.assertFalse(receipt["success_verdict"])
        self.assertEqual(receipt["failures"], [])

    def test_json_format_drift_is_stream_invalid(self):
        lines = [
            envelope("pkg_a"),
            '{"type": "suite", "event": "start',  # drifted/truncated JSON event
            run_end("pkg_a", 0),
        ]
        receipt = self.parse(lines)
        self.assertEqual(receipt["verdict"], "STREAM_INVALID")
        self.assertNotEqual(receipt["exit_code"], 0)

    def test_truncated_stream_is_stream_incomplete(self):
        lines = [
            envelope("pkg_a"),
            suite_start(2),
            test_event("started", "a::one"),
            test_event("ok", "a::one"),
            # stream cut off: no final suite event, no run_end
        ]
        receipt = self.parse(lines)
        self.assertEqual(receipt["verdict"], "STREAM_INCOMPLETE")
        self.assertFalse(receipt["success_verdict"])

    def test_missing_run_end_trailer_is_stream_incomplete(self):
        lines = [
            envelope("pkg_a"),
            suite_start(1),
            test_event("started", "a::one"),
            test_event("ok", "a::one"),
            suite_end("ok", 1, 0),
        ]
        receipt = self.parse(lines)
        self.assertEqual(receipt["verdict"], "STREAM_INCOMPLETE")

    def test_empty_stream_is_stream_invalid_not_pass(self):
        receipt = self.parse([])
        self.assertEqual(receipt["verdict"], "STREAM_INVALID")
        self.assertEqual(receipt["exit_code"], 3)
        self.assertEqual(receipt["counts"]["failed"], 0)

    def test_diagnostics_only_zero_exit_stream_is_stream_invalid(self):
        lines = [
            envelope("pkg_a"),
            "warning: only non-test diagnostics were emitted",
            run_end("pkg_a", 0),
        ]
        receipt = self.parse(lines)
        self.assertEqual(receipt["verdict"], "STREAM_INVALID")

    def test_zero_discovered_tests_is_not_a_complete_pass(self):
        lines = [
            envelope("pkg_a"),
            suite_start(0),
            suite_end("ok", 0, 0),
            run_end("pkg_a", 0),
        ]
        receipt = self.parse(lines)
        self.assertEqual(receipt["verdict"], "NO_TESTS_DISCOVERED")
        self.assertIn(
            "NO_TESTS_DISCOVERED", _MODULE.EXIT_CODES
        )  # typed state, gated by the workflow for unfiltered runs

    def test_valid_all_pass_stream_reconciles(self):
        lines = [
            envelope("pkg_a"),
            suite_start(2),
            test_event("started", "a::one"),
            test_event("ok", "a::one"),
            test_event("started", "a::two"),
            test_event("ignored", "a::two"),
            suite_end("ok", 1, 0, 1),
            run_end("pkg_a", 0),
        ]
        receipt = self.parse(lines)
        self.assertEqual(receipt["verdict"], "COMPLETE_PASS")
        self.assertEqual(receipt["counts"]["passed"], 1)
        self.assertEqual(receipt["counts"]["ignored"], 1)
        self.assertEqual(receipt["counts"]["failed"], 0)

    def test_complete_with_failures_records_exact_identity(self):
        lines = [
            envelope("pkg_a"),
            suite_start(2),
            test_event("started", "a::flaky_case"),
            test_event("failed", "a::flaky_case"),
            test_event("ok", "a::other"),
            suite_end("failed", 1, 1),
            run_end("pkg_a", 101),
        ]
        receipt = self.parse(lines)
        self.assertEqual(receipt["verdict"], "COMPLETE_WITH_FAILURES")
        self.assertEqual(receipt["exit_code"], 0)
        self.assertEqual(len(receipt["failures"]), 1)
        failure = receipt["failures"][0]
        self.assertEqual(failure["package"], "pkg_a")
        self.assertEqual(failure["target"], "lib")
        self.assertEqual(failure["name"], "a::flaky_case")

    def test_duplicate_test_names_in_two_packages_stay_distinct(self):
        lines = [
            envelope("pkg_a"),
            suite_start(1),
            test_event("failed", "common::case"),
            suite_end("failed", 0, 1),
            run_end("pkg_a", 101),
            envelope("pkg_b"),
            suite_start(1),
            test_event("failed", "common::case"),
            suite_end("failed", 0, 1),
            run_end("pkg_b", 101),
        ]
        receipt = self.parse(lines)
        self.assertEqual(receipt["verdict"], "COMPLETE_WITH_FAILURES")
        packages = sorted(f["package"] for f in receipt["failures"])
        self.assertEqual(packages, ["pkg_a", "pkg_b"])
        self.assertEqual(
            [(f["package"], f["name"]) for f in receipt["failures"]],
            [("pkg_a", "common::case"), ("pkg_b", "common::case")],
        )

    def test_suite_ok_with_failed_events_is_not_proven(self):
        lines = [
            envelope("pkg_a"),
            suite_start(2),
            test_event("failed", "a::one"),
            test_event("ok", "a::two"),
            suite_end("ok", 1, 1),
            run_end("pkg_a", 101),
        ]
        receipt = self.parse(lines)
        self.assertEqual(receipt["verdict"], "NOT_PROVEN")

    def test_suite_failed_but_zero_exit_is_not_proven(self):
        lines = [
            envelope("pkg_a"),
            suite_start(1),
            test_event("failed", "a::one"),
            suite_end("failed", 0, 1),
            run_end("pkg_a", 0),
        ]
        receipt = self.parse(lines)
        self.assertEqual(receipt["verdict"], "NOT_PROVEN")

    def test_summary_count_mismatch_is_not_proven(self):
        lines = [
            envelope("pkg_a"),
            suite_start(5),
            test_event("ok", "a::one"),
            suite_end("failed", 5, 0),
            run_end("pkg_a", 0),
        ]
        receipt = self.parse(lines)
        self.assertEqual(receipt["verdict"], "NOT_PROVEN")

    def test_mid_block_corruption_is_not_overwritten_by_valid_suite(self):
        lines = [
            envelope("pkg_a"),
            suite_start(2),
            test_event("ok", "a::one"),
            '{"type": "test", "event": "failed", "name": "a::two"',  # corrupted
            test_event("failed", "a::two"),
            suite_end("failed", 1, 1),
            run_end("pkg_a", 101),
        ]
        receipt = self.parse(lines)
        self.assertEqual(receipt["verdict"], "STREAM_INVALID")

    def test_stderr_noise_inside_valid_block_does_not_invalidate(self):
        lines = [
            envelope("pkg_a"),
            suite_start(2),
            test_event("started", "a::one"),
            "logging noise from the test process",
            test_event("failed", "a::one"),
            test_event("ok", "a::two"),
            suite_end("failed", 1, 1),
            run_end("pkg_a", 101),
        ]
        receipt = self.parse(lines)
        self.assertEqual(receipt["verdict"], "COMPLETE_WITH_FAILURES")
        self.assertEqual(receipt["counts"]["noise_lines"], 1)

    def test_unreadable_input_is_extractor_failed(self):
        with tempfile.TemporaryDirectory() as tmp:
            missing = str(Path(tmp) / "does-not-exist.jsonl")
            buffer = io.StringIO()
            with contextlib.redirect_stdout(buffer):
                code = _MODULE.main([missing, "--head", "deadbeef"])
            receipt = json.loads(buffer.getvalue())
        self.assertEqual(code, 5)
        self.assertEqual(receipt["verdict"], "EXTRACTOR_FAILED")
        self.assertFalse(receipt["success_verdict"])

    def test_cli_exit_code_and_receipt_metadata_end_to_end(self):
        with tempfile.TemporaryDirectory() as tmp:
            stream = Path(tmp) / "obs.jsonl"
            stream.write_text(
                "\n".join(
                    [
                        envelope("pkg_a"),
                        "error: could not compile `pkg_a`",
                        run_end("pkg_a", 101),
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            proc = subprocess.run(
                [
                    sys.executable,
                    str(_MODULE_PATH),
                    str(stream),
                    "--head",
                    "abc123",
                    "--toolchain",
                    "cargo 1.90.0",
                ],
                capture_output=True,
                text=True,
            )
        self.assertEqual(proc.returncode, 2)
        receipt = json.loads(proc.stdout)
        self.assertEqual(receipt["verdict"], "COMMAND_FAILED")
        self.assertEqual(receipt["exit_code"], proc.returncode)
        self.assertEqual(receipt["metadata"]["head"], "abc123")
        self.assertEqual(receipt["metadata"]["toolchain"], "cargo 1.90.0")
        self.assertEqual(receipt["metadata"]["observed_target_families"], ["lib"])

    def test_success_verdict_exit_is_zero_end_to_end(self):
        with tempfile.TemporaryDirectory() as tmp:
            stream = Path(tmp) / "obs.jsonl"
            stream.write_text(
                "\n".join(
                    [
                        envelope("pkg_a"),
                        suite_start(1),
                        test_event("started", "a::one"),
                        test_event("ok", "a::one"),
                        suite_end("ok", 1, 0),
                        run_end("pkg_a", 0),
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            proc = subprocess.run(
                [sys.executable, str(_MODULE_PATH), str(stream)],
                capture_output=True,
                text=True,
            )
        self.assertEqual(proc.returncode, 0)
        self.assertEqual(json.loads(proc.stdout)["verdict"], "COMPLETE_PASS")


if __name__ == "__main__":
    unittest.main(verbosity=2)
