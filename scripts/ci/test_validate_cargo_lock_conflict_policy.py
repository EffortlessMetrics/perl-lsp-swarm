#!/usr/bin/env python3
"""Focused tests for the Cargo.lock conflict-repair source contract."""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

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

    def test_malformed_unhashable_context_or_command_is_not_proven(self) -> None:
        for case in (
            {"context": ["conflict_repair"], "command": "cargo update"},
            {"context": "conflict_repair", "command": ["cargo update"]},
        ):
            with self.subTest(case=case):
                self.assertEqual(validator.classify(case), "not_proven")

    def test_malformed_fixture_entries_return_deterministic_errors(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            fixture = root / validator.FIXTURE
            fixture.parent.mkdir(parents=True)
            fixture.write_text('{"cases": [null, ["not a mapping"]]}', encoding="utf-8")
            errors = validator.validate(root)
            self.assertEqual(
                errors[:2],
                ["cases[0] must be a mapping", "cases[1] must be a mapping"],
            )

    def test_weakened_policy_text_is_rejected(self) -> None:
        errors = validator.validate_semantics(
            "Active conflict repair may use `cargo generate-lockfile`.\n",
            1,
            {"required": ["must not use `cargo generate-lockfile`"], "forbidden": []},
        )
        self.assertTrue(errors)

    def test_empty_semantic_assertion_strings_are_rejected(self) -> None:
        for semantics, message in (
            ({"required": [" \t\n"], "forbidden": []}, "required source semantics must not be empty"),
            ({"required": [], "forbidden": [" \t\n"]}, "forbidden source semantics must not be empty"),
        ):
            with self.subTest(semantics=semantics):
                errors = validator.validate_semantics("anchor\n", 1, semantics)
                self.assertIn(message, errors)

    def test_empty_semantic_assertions_are_rejected(self) -> None:
        errors = validator.validate_semantics("anchor\n", 1, {"required": [], "forbidden": []})
        self.assertIn("must not both be empty", errors[0])

    def test_non_string_forbidden_assertions_are_rejected(self) -> None:
        errors = validator.validate_semantics("anchor\n", 1, {"required": ["anchor"], "forbidden": [7]})
        self.assertIn("forbidden source semantics must be strings", errors[0])

    def test_non_string_required_assertions_are_rejected(self) -> None:
        errors = validator.validate_semantics("anchor\n", 1, {"required": [7], "forbidden": []})
        self.assertIn("required source semantics must be strings", errors[0])

    def test_nearby_text_cannot_satisfy_anchor_semantics(self) -> None:
        errors = validator.validate_semantics(
            "required text on another line\nanchor\n", 2, {"required": ["required text"]}
        )
        self.assertTrue(errors)

    def test_anchor_line_is_derived_from_unique_needle(self) -> None:
        source = "intro\nanchor statement\ntrailing\n"
        self.assertEqual(validator.anchor_line(source, "anchor statement"), 2)
        self.assertIsNone(validator.anchor_line(source, "missing"))

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

    def test_temporary_lock_mutation_between_reads_is_not_proven(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            lock = Path(temp) / "Cargo.lock"
            original = b"accepted\n"
            mutated = b"mutated\n"
            lock.write_bytes(original)
            real_read_bytes = Path.read_bytes
            reads = 0

            def read_with_temporary_mutation(path: Path) -> bytes:
                nonlocal reads
                reads += 1
                contents = real_read_bytes(path)
                if reads == 1:
                    path.write_bytes(mutated)
                return contents

            with patch.object(Path, "read_bytes", read_with_temporary_mutation):
                result = validator.validate_transition(
                    original,
                    original,
                    manifest_requires_lock=False,
                    temporary_lock_path=lock,
                )
            self.assertEqual(result, "not_proven")
            self.assertEqual(reads, 2)
            self.assertEqual(lock.read_bytes(), mutated)

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
