#!/usr/bin/env python3
"""Focused falsifiers for source-specific GitHub enforcement bindings."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import sys
import unittest
from pathlib import Path
from typing import Any

SCRIPT = Path(__file__).with_name("validate_gate_enforcement_contract.py")
SPEC = importlib.util.spec_from_file_location("gate_enforcement_contract_bindings", SCRIPT)
assert SPEC and SPEC.loader
contract = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = contract
SPEC.loader.exec_module(contract)


def required_entry(enforcement: str, **overrides: Any) -> dict[str, Any]:
    entry: dict[str, Any] = {
        "name": "Bound Check",
        "producer": "external",
        "workflow": "github-app",
        "required": True,
        "policy_role": "required",
        "applicability": "always-or-scoped-noop",
        "enforcement": enforcement,
    }
    entry.update(overrides)
    return entry


def finding_codes(entry: dict[str, Any]) -> set[str]:
    findings, _ = contract.validate_context(entry, {}, {})
    return {finding.code for finding in findings}


def canonical_digest(entry: dict[str, Any]) -> str:
    payload = json.dumps(
        contract._canonical_context(entry),
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


class AppBindingContractTests(unittest.TestCase):
    def test_valid_source_specific_bindings_are_accepted(self) -> None:
        cases = (
            required_entry(
                "github-branch-protection",
                classic_app_id=15368,
            ),
            required_entry(
                "github-ruleset",
                ruleset_integration_id=15368,
            ),
            required_entry(
                "github-branch-protection+ruleset",
                classic_app_id=15368,
                ruleset_integration_id=15368,
            ),
        )
        for entry in cases:
            with self.subTest(entry=entry):
                self.assertEqual(set(), finding_codes(entry))

    def test_canonical_subject_retains_explicit_bindings(self) -> None:
        canonical = contract._canonical_context(
            required_entry(
                "github-branch-protection+ruleset",
                classic_app_id=15368,
                ruleset_integration_id=4242,
            )
        )
        self.assertEqual(15368, canonical["classic_app_id"])
        self.assertEqual(4242, canonical["ruleset_integration_id"])

    def test_absent_bindings_are_not_synthesized_from_producer(self) -> None:
        entry = required_entry("github-ruleset", producer="repository-job")
        canonical = contract._canonical_context(entry)
        self.assertNotIn("classic_app_id", canonical)
        self.assertNotIn("ruleset_integration_id", canonical)

    def test_invalid_binding_values_fail_closed(self) -> None:
        for field in ("classic_app_id", "ruleset_integration_id"):
            enforcement = (
                "github-branch-protection"
                if field == "classic_app_id"
                else "github-ruleset"
            )
            for value in (0, -1, "15368", 15368.0, True, None):
                with self.subTest(field=field, value=value):
                    entry = required_entry(enforcement, **{field: value})
                    self.assertIn(f"invalid_{field}", finding_codes(entry))

    def test_unknown_binding_aliases_fail_closed(self) -> None:
        for field in ("app_id", "classic_appid", "ruleset_app_id"):
            with self.subTest(field=field):
                entry = required_entry(
                    "github-branch-protection",
                    **{field: 15368},
                )
                self.assertIn("unknown_context_field", finding_codes(entry))

    def test_classic_binding_requires_classic_enforcement(self) -> None:
        entry = required_entry("github-ruleset", classic_app_id=15368)
        self.assertIn(
            "classic_app_id_source_mismatch",
            finding_codes(entry),
        )

    def test_ruleset_binding_requires_ruleset_enforcement(self) -> None:
        entry = required_entry(
            "github-branch-protection",
            ruleset_integration_id=15368,
        )
        self.assertIn(
            "ruleset_integration_id_source_mismatch",
            finding_codes(entry),
        )

    def test_binding_change_changes_static_subject_identity(self) -> None:
        first = required_entry(
            "github-branch-protection",
            classic_app_id=15368,
        )
        second = required_entry(
            "github-branch-protection",
            classic_app_id=4242,
        )
        self.assertNotEqual(canonical_digest(first), canonical_digest(second))


if __name__ == "__main__":
    unittest.main()
