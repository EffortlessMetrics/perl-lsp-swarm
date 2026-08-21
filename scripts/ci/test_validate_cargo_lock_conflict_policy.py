#!/usr/bin/env python3
"""Focused tests for the Cargo.lock conflict-repair source contract."""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("validate_cargo_lock_conflict_policy.py")
SPEC = importlib.util.spec_from_file_location("cargo_lock_policy", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
validator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(validator)

ROOT = Path(__file__).resolve().parents[2]


class CargoLockConflictPolicyTests(unittest.TestCase):
    def test_current_fixture_and_source_anchors_are_valid(self) -> None:
        self.assertEqual(validator.validate(ROOT), [])

    def test_dynamic_construction_is_not_proven(self) -> None:
        self.assertEqual(
            validator.classify({"context": "dynamic", "command": None}),
            "not_proven",
        )

    def test_compatible_accepted_lock_is_byte_identical(self) -> None:
        original = b"accepted\n"
        self.assertEqual(
            validator.validate_transition(original, original, manifest_requires_lock=False),
            "accepted_lock_preserved",
        )

    def test_manifest_required_change_refuses_without_mutation(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            lock = Path(temp) / "Cargo.lock"
            original = b"accepted\n"
            lock.write_bytes(original)
            self.assertEqual(
                validator.validate_transition(original, b"new\n", manifest_requires_lock=True),
                "manifest_requires_lock_change",
            )
            self.assertEqual(lock.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
