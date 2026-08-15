#!/usr/bin/env python3
"""Focused tests for check_publish_package_contents.py."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("check_publish_package_contents.py")
SPEC = importlib.util.spec_from_file_location("check_publish_package_contents", SCRIPT_PATH)
assert SPEC is not None
check = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(check)


class PublishPackageContentsTests(unittest.TestCase):
    def test_ignored_regression_file_is_rejected(self) -> None:
        package_files = {
            "Cargo.toml",
            "src/lib.rs",
            "tests/_proptest-regressions/lexer.proptest-regressions",
        }
        tracked_files = {"Cargo.toml", "src/lib.rs"}

        self.assertEqual(
            check.unexpected_packaged_files(package_files, tracked_files),
            ["tests/_proptest-regressions/lexer.proptest-regressions"],
        )

    def test_cargo_generated_files_and_workspace_lockfile_are_allowed(self) -> None:
        package_files = {".cargo_vcs_info.json", "Cargo.lock", "Cargo.toml.orig", "Cargo.toml"}
        tracked_files = {"Cargo.lock", "Cargo.toml"}

        self.assertEqual(check.unexpected_packaged_files(package_files, tracked_files), [])

    def test_parent_traversal_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            check.normalize_package_path("tests/../secret.txt")

    def test_windows_paths_are_normalized(self) -> None:
        self.assertEqual(check.normalize_package_path(r"tests\fixture.txt"), "tests/fixture.txt")


if __name__ == "__main__":
    unittest.main()
