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

    def test_contexts_do_not_override_forbidden_or_unknown_commands(self) -> None:
        for context in ("release_refresh", "targeted_dependency", "isolated_extracted_package"):
            self.assertEqual(
                validator.classify({"context": context, "command": "cargo update"}),
                "not_proven",
            )
            self.assertEqual(
                validator.classify({"context": context, "command": "dynamic command"}),
                "not_proven",
            )

    def test_marker_without_source_prohibition_is_rejected(self) -> None:
        errors = validator.validate_semantics(
            "marker\nactive conflict repair: cargo generate-lockfile\nweakened text\n",
            2,
            {"required": ["must not use `cargo generate-lockfile`"], "forbidden": []},
        )
        self.assertTrue(errors)

    def test_empty_semantic_assertions_are_rejected(self) -> None:
        errors = validator.validate_semantics("anchor\n", 1, {"required": [], "forbidden": []})
        self.assertIn("must not both be empty", errors[0])

    def test_non_string_forbidden_assertions_are_rejected(self) -> None:
        errors = validator.validate_semantics("anchor\n", 1, {"required": ["anchor"], "forbidden": [7]})
        self.assertIn("forbidden source semantics must be strings", errors[0])

    def test_nearby_text_cannot_satisfy_anchor_semantics(self) -> None:
        errors = validator.validate_semantics(
            "required text on another line\nanchor\n", 2, {"required": ["required text"]}
        )
        self.assertTrue(errors)

    def test_compatible_accepted_lock_is_byte_identical(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            lock = Path(temp) / "Cargo.lock"
            original = b"accepted\n"
            lock.write_bytes(original)
            self.assertEqual(
                validator.validate_transition(
                    original,
                    original,
                    manifest_requires_lock=False,
                    temporary_lock_path=lock,
                ),
                "accepted_lock_preserved",
            )
            self.assertEqual(lock.read_bytes(), original)

    def test_missing_proposed_lock_is_not_proven(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            lock = Path(temp) / "Cargo.lock"
            original = b"accepted\n"
            lock.write_bytes(original)
            self.assertEqual(
                validator.validate_transition(
                    original,
                    None,
                    manifest_requires_lock=False,
                    temporary_lock_path=lock,
                ),
                "not_proven",
            )

    def test_differing_lock_requires_admission(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            lock = Path(temp) / "Cargo.lock"
            original = b"accepted\n"
            lock.write_bytes(original)
            self.assertEqual(
                validator.validate_transition(
                    original,
                    b"different\n",
                    manifest_requires_lock=False,
                    temporary_lock_path=lock,
                ),
                "lock_conflict_requires_admission",
            )
            self.assertEqual(lock.read_bytes(), original)

    def test_manifest_required_change_refuses_without_mutation(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            lock = Path(temp) / "Cargo.lock"
            original = b"accepted\n"
            lock.write_bytes(original)
            self.assertEqual(
                validator.validate_transition(
                    original,
                    b"new\n",
                    manifest_requires_lock=True,
                    temporary_lock_path=lock,
                ),
                "manifest_requires_lock_change",
            )
            self.assertEqual(lock.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
