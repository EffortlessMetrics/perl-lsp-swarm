#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import os
import sys
import unittest
from pathlib import Path
from unittest import mock

MODULE_PATH = Path(__file__).with_name("validate_paths.py")
SPEC = importlib.util.spec_from_file_location("upload_receipt_validate_paths", MODULE_PATH)
assert SPEC and SPEC.loader
validate_paths = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = validate_paths
SPEC.loader.exec_module(validate_paths)


def environment(**overrides: str) -> dict[str, str]:
    """A valid baseline environment, mirroring the action's declared defaults."""
    values = {
        "RECEIPT_PATH": "target/receipts/receipt.json",
        "LOGS_PATH": "target/receipts/logs",
        "ARTIFACTS_PATH": "target/receipts/artifacts",
    }
    values.update(overrides)
    return values


class ValidatePathsTests(unittest.TestCase):
    def test_declared_defaults_are_accepted(self) -> None:
        self.assertEqual(validate_paths.collect_errors(environment()), [])

    def test_runner_temp_path_outside_workspace_is_accepted(self) -> None:
        """The in-repo caller uploads from `runner.temp`, not the workspace."""
        env = environment(RECEIPT_PATH="/home/runner/work/_temp/droid-receipts/r.json")
        self.assertEqual(validate_paths.collect_errors(env), [])

    def test_newline_is_rejected_for_every_path_input(self) -> None:
        """An LF would append extra entries to upload-artifact's path list."""
        for env_name in ("RECEIPT_PATH", "LOGS_PATH", "ARTIFACTS_PATH"):
            with self.subTest(env_name=env_name):
                env = environment(
                    **{env_name: "target/receipt.json\n/home/runner/.config"}
                )
                errors = validate_paths.collect_errors(env)
                self.assertEqual(len(errors), 1)
                self.assertIn("U+000A", errors[0])

    def test_carriage_return_is_rejected(self) -> None:
        env = environment(LOGS_PATH="target/logs\r/etc")
        errors = validate_paths.collect_errors(env)
        self.assertEqual(len(errors), 1)
        self.assertIn("U+000D", errors[0])

    def test_empty_and_whitespace_only_values_are_rejected(self) -> None:
        for value in ("", "   "):
            with self.subTest(value=value):
                errors = validate_paths.collect_errors(environment(RECEIPT_PATH=value))
                self.assertEqual(len(errors), 1)
                self.assertIn("must not be empty", errors[0])

    def test_missing_variable_is_rejected_rather_than_defaulted(self) -> None:
        env = environment()
        del env["ARTIFACTS_PATH"]
        errors = validate_paths.collect_errors(env)
        self.assertEqual(len(errors), 1)
        self.assertIn("artifacts-path", errors[0])

    def test_every_invalid_input_is_reported(self) -> None:
        env = environment(RECEIPT_PATH="a\nb", LOGS_PATH="", ARTIFACTS_PATH="c\rd")
        self.assertEqual(len(validate_paths.collect_errors(env)), 3)

    def test_main_exits_nonzero_only_when_invalid(self) -> None:
        with mock.patch.dict(os.environ, environment(), clear=True):
            self.assertEqual(validate_paths.main([]), 0)
        with mock.patch.dict(os.environ, environment(RECEIPT_PATH="a\nb"), clear=True):
            self.assertEqual(validate_paths.main([]), 1)


if __name__ == "__main__":
    unittest.main()
