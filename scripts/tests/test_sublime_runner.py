"""Falsifiers for scripts/sublime/run_unit_tests.py verdict logic."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/sublime/run_unit_tests.py"
SPEC = importlib.util.spec_from_file_location("run_unit_tests", SCRIPT)
assert SPEC and SPEC.loader
runner = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = runner
SPEC.loader.exec_module(runner)


class ReadOutputTests(unittest.TestCase):
    def setUp(self) -> None:
        self._paths: list[Path] = []

    def tearDown(self) -> None:
        for path in self._paths:
            path.unlink(missing_ok=True)

    def _read(self, lines: list[str]) -> bool | None:
        handle = tempfile.NamedTemporaryFile(mode="w", delete=False, encoding="utf-8")
        handle.write("".join(f"{line}\n" for line in lines + ["UnitTesting: Done."]))
        handle.close()
        path = Path(handle.name)
        self._paths.append(path)
        return runner._read_output(str(path), idle_timeout=5)

    def test_ok_without_ran_summary_is_not_success(self) -> None:
        self.assertFalse(self._read(["OK"]))

    def test_ran_zero_with_ok_is_not_success(self) -> None:
        self.assertFalse(self._read(["Ran 0 tests in 0.000s", "OK"]))

    def test_ran_positive_with_ok_is_success(self) -> None:
        self.assertTrue(self._read(["Ran 2 tests in 0.001s", "OK"]))

    def test_ran_positive_with_ok_and_skips_is_success(self) -> None:
        self.assertTrue(self._read(["Ran 3 tests in 0.010s", "OK (skipped=1)"]))

    def test_ran_one_with_ok_and_skipped_subtest_is_success(self) -> None:
        self.assertTrue(self._read(["Ran 1 test in 0.001s", "OK (skipped=1)"]))

    def test_failed_verdict_is_not_success(self) -> None:
        self.assertFalse(self._read(["Ran 2 tests in 0.001s", "FAILED (failures=1)"]))

    def test_error_verdict_is_not_success(self) -> None:
        self.assertFalse(self._read(["Ran 1 test in 0.001s", "ERROR"]))

    def test_main_exit_decision_matches_reader_for_zero_ran(self) -> None:
        success = self._read(["Ran 0 tests in 0.000s", "OK"])
        self.assertEqual(1, 0 if success is True else 1)

    def test_main_exit_decision_matches_reader_for_executed_suite(self) -> None:
        success = self._read(["Ran 2 tests in 0.001s", "OK"])
        self.assertEqual(0, 0 if success is True else 1)


if __name__ == "__main__":
    unittest.main()
