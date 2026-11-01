"""Focused tests for ``scripts/sublime/run_unit_tests.py`` result parsing."""

from __future__ import annotations

import importlib.util
import os
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RUNNER_PATH = ROOT / "scripts/sublime/run_unit_tests.py"

_SPEC = importlib.util.spec_from_file_location("run_unit_tests", RUNNER_PATH)
assert _SPEC is not None and _SPEC.loader is not None
runner = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(runner)


class SublimeRunnerResultTests(unittest.TestCase):
    def _read_lines(self, lines: list[str]) -> bool | None:
        payload = "".join(line if line.endswith("\n") else f"{line}\n" for line in lines)
        with tempfile.NamedTemporaryFile("w", encoding="utf-8", delete=False) as handle:
            handle.write(payload)
            path = handle.name
        try:
            return runner._read_output(path, idle_timeout=5)
        finally:
            os.unlink(path)

    def test_ok_without_ran_summary_fails(self) -> None:
        self.assertFalse(
            self._read_lines(["OK", "UnitTesting: Done."])
        )

    def test_ran_zero_ok_fails(self) -> None:
        self.assertFalse(
            self._read_lines(
                ["Ran 0 tests in 0.000s", "OK", "UnitTesting: Done."]
            )
        )

    def test_ran_three_ok_passes(self) -> None:
        self.assertTrue(
            self._read_lines(
                ["Ran 3 tests in 0.012s", "OK", "UnitTesting: Done."]
            )
        )

    def test_ran_three_ok_with_skipped_annotation_passes(self) -> None:
        self.assertTrue(
            self._read_lines(
                [
                    "Ran 3 tests in 0.012s",
                    "OK (skipped=1)",
                    "UnitTesting: Done.",
                ]
            )
        )

    def test_failed_verdict_fails(self) -> None:
        self.assertFalse(
            self._read_lines(
                [
                    "Ran 2 tests in 0.010s",
                    "FAILED (failures=1)",
                    "UnitTesting: Done.",
                ]
            )
        )

    def test_error_verdict_fails(self) -> None:
        self.assertFalse(
            self._read_lines(
                ["Ran 1 test in 0.001s", "ERROR", "UnitTesting: Done."]
            )
        )

    def test_ran_one_ok_passes(self) -> None:
        self.assertTrue(
            self._read_lines(
                ["Ran 1 test in 0.001s", "OK", "UnitTesting: Done."]
            )
        )

    def test_ok_before_ran_line_still_passes(self) -> None:
        self.assertTrue(
            self._read_lines(
                ["OK", "Ran 2 tests in 0.002s", "UnitTesting: Done."]
            )
        )

    def test_plural_ran_summary_parses(self) -> None:
        self.assertTrue(
            self._read_lines(
                ["Ran 2 tests in 0.002s", "OK", "UnitTesting: Done."]
            )
        )


if __name__ == "__main__":
    unittest.main()
