"""Focused tests for scripts/sublime/run_unit_tests.py verdict logic."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
RUNNER_PATH = ROOT / "scripts" / "sublime" / "run_unit_tests.py"

_spec = importlib.util.spec_from_file_location("run_unit_tests", RUNNER_PATH)
assert _spec and _spec.loader
runner = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = runner
_spec.loader.exec_module(runner)


def _write_result(path: Path, body: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(body if body.endswith("\n") else body + "\n", encoding="utf-8")


class ReadOutputTests(unittest.TestCase):
    def run_reader(self, body: str) -> bool | None:
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "result"
            _write_result(path, body)
            return runner._read_output(str(path), idle_timeout=5)

    def test_ok_without_ran_summary_fails(self) -> None:
        self.assertFalse(
            self.run_reader("OK\nUnitTesting: Done.\n"),
        )

    def test_ran_zero_ok_fails(self) -> None:
        self.assertFalse(
            self.run_reader(
                "Ran 0 tests in 0.001s\n\nOK\nUnitTesting: Done.\n",
            ),
        )

    def test_ran_three_ok_with_skip_passes(self) -> None:
        self.assertTrue(
            self.run_reader(
                "Ran 3 tests in 0.010s\n\nOK (skipped=1)\nUnitTesting: Done.\n",
            ),
        )

    def test_failed_terminal_fails(self) -> None:
        self.assertFalse(
            self.run_reader(
                "Ran 1 test in 0.001s\n\nFAILED (failures=1)\nUnitTesting: Done.\n",
            ),
        )

    def test_error_terminal_fails(self) -> None:
        self.assertFalse(
            self.run_reader(
                "Ran 1 test in 0.001s\n\nERROR\nUnitTesting: Done.\n",
            ),
        )

    def test_ran_one_test_singular_ok_passes(self) -> None:
        self.assertTrue(
            self.run_reader(
                "Ran 1 test in 0.001s\n\nOK\nUnitTesting: Done.\n",
            ),
        )


class MainExitTests(unittest.TestCase):
    def run_main_with_fixture(self, body: str) -> int:
        with tempfile.TemporaryDirectory() as temp:
            packages = Path(temp) / "packages"
            package = "LSP-perllsp"
            output_dir = packages / "User" / "UnitTesting" / package
            output_file = output_dir / "result"
            _write_result(output_file, body)
            schedule_path = packages / "User" / "UnitTesting" / "schedule.json"
            schedule_path.parent.mkdir(parents=True, exist_ok=True)
            schedule_path.write_text("[]", encoding="utf-8")

            argv = ["run_unit_tests.py", package]

            def fake_wait(path: str, timeout: int) -> None:
                del timeout
                _write_result(Path(path), body)

            patches = (
                mock.patch.object(runner, "PACKAGES_DIR_PATH", str(packages)),
                mock.patch.object(
                    runner,
                    "UT_OUTPUT_DIR_PATH",
                    str(packages / "User" / "UnitTesting"),
                ),
                mock.patch.object(
                    runner,
                    "SCHEDULE_FILE_PATH",
                    str(schedule_path),
                ),
                mock.patch.object(runner, "OUTPUT_IDLE_TIMEOUT_SECONDS", 2),
                mock.patch.object(runner, "_start_sublime_text"),
                mock.patch.object(runner, "_kill_sublime_text"),
                mock.patch.object(runner, "_wait_for_output", fake_wait),
                mock.patch.object(sys, "argv", argv),
            )
            with patches[0], patches[1], patches[2], patches[3], patches[4], patches[5], patches[6], patches[7]:
                return runner.main()

    def test_main_fails_on_zero_test_ok(self) -> None:
        self.assertEqual(
            self.run_main_with_fixture(
                "Ran 0 tests in 0.001s\n\nOK\nUnitTesting: Done.\n",
            ),
            1,
        )

    def test_main_passes_on_executed_tests(self) -> None:
        self.assertEqual(
            self.run_main_with_fixture(
                "Ran 2 tests in 0.002s\n\nOK\nUnitTesting: Done.\n",
            ),
            0,
        )


if __name__ == "__main__":
    unittest.main()
