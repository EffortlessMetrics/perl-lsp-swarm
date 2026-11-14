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



class ReporterBindingTests(unittest.TestCase):
    def binding(self, **changes):
        return dict(kind="check-run", app_id=15368,
                    obligation_ids=["gate-a", "gate-b"], coverage_complete=True, **changes)

    def entry(self, binding):
        return required_entry("github-ruleset", ruleset_integration_id=15368,
                              reporter_binding=binding)

    def test_explicit_check_run_binding_and_legacy_creator_are_separate(self):
        self.assertEqual(set(), finding_codes(self.entry(self.binding())))
        legacy = required_entry("neither", required=False, policy_role="advisory",
            reporter_binding=dict(kind="legacy-status", creator_id=777,
                obligation_ids=["external-a"], coverage_complete=True))
        self.assertEqual(set(), finding_codes(legacy))
        legacy["reporter_binding"]["app_id"] = 777
        self.assertIn("invalid_reporter_binding", finding_codes(legacy))

    def test_missing_fields_wrong_types_and_incomplete_coverage_refuse(self):
        for field in self.binding():
            binding=self.binding();del binding[field]
            with self.subTest(missing=field):
                self.assertIn("invalid_reporter_binding", finding_codes(self.entry(binding)))
        for field, values in {
            "kind": [None, [], "unknown"], "app_id": [True, 0, -1, "15368"],
            "obligation_ids": [None, [], [""], [" x"], [1], ["x", "x"]],
            "coverage_complete": [False, None, 1, "true"],
        }.items():
            for value in values:
                binding=self.binding();binding[field]=value
                with self.subTest(field=field,value=value):
                    self.assertIn("invalid_reporter_binding", finding_codes(self.entry(binding)))

    def test_conflicting_identity_unknown_fields_and_wrong_app_refuse(self):
        for changes in ({"creator_id":777},{"app_id":777},{"unexpected":"x"}):
            binding=self.binding();binding.update(changes)
            self.assertIn("invalid_reporter_binding", finding_codes(self.entry(binding)))
        entry=self.entry(self.binding());entry["classic_app_id"]=999
        self.assertIn("invalid_reporter_binding", finding_codes(entry))

    def test_absence_retains_old_contract_without_claiming_provenance(self):
        entry=required_entry("github-ruleset",ruleset_integration_id=15368)
        self.assertEqual(set(),finding_codes(entry))
        self.assertNotIn("reporter_binding",contract._canonical_context(entry))
        for binding in (None, [], False):
            self.assertIn("invalid_reporter_binding",finding_codes(self.entry(binding)))

    def test_legacy_cannot_borrow_app_binding_or_repository_job(self):
        binding=dict(kind="legacy-status",creator_id=15368,
            obligation_ids=["a"],coverage_complete=True)
        self.assertIn("invalid_reporter_binding",finding_codes(self.entry(binding)))
        entry=required_entry("neither",required=False,policy_role="advisory",
            producer="repository-job",reporter_binding=binding)
        self.assertIn("invalid_reporter_binding",finding_codes(entry))

    def test_canonical_coverage_order_and_identity_movement(self):
        entry=self.entry(self.binding());before=canonical_digest(entry)
        entry["reporter_binding"]["obligation_ids"].reverse()
        self.assertEqual(before,canonical_digest(entry))
        for field,value in (("app_id",777),("obligation_ids",["gate-a"]),
                            ("coverage_complete",False),("kind","legacy-status")):
            other=self.binding();other[field]=value
            self.assertNotEqual(before,canonical_digest(self.entry(other)))
        self.assertEqual(["gate-b","gate-a"],entry["reporter_binding"]["obligation_ids"])

    def test_real_git_fixture_binds_complete_declaration_to_semantic_subject(self):
        import tempfile
        from scripts.ci.test_validate_gate_enforcement_contract import fixture, REQUIRED
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            policy=fixture(root, REQUIRED + '\n[checks.reporter_binding]\n'
                'kind="check-run"\napp_id=15368\n'
                'obligation_ids=["gate-b","gate-a"]\ncoverage_complete=true\n')
            result=contract.validate(root,policy)
            self.assertEqual("SUCCESS",result["status"])
            self.assertEqual("NOT_PROVEN",result["live_enforcement_status"])
            binding=result["semantic_subject"]["contexts"][0]["reporter_binding"]
            self.assertEqual(["gate-a","gate-b"],binding["obligation_ids"])
            policy.write_text(policy.read_text().replace('"gate-b","gate-a"','"gate-a"'))
            # Commit the isolated fixture change: validate() refuses dirty trees.
            import subprocess
            subprocess.run(["git","-C",str(root),"add","--",str(policy)],check=True,capture_output=True)
            subprocess.run(["git","-C",str(root),"commit","-qm","change coverage"],check=True,capture_output=True)
            changed=contract.validate(root,policy)
            self.assertEqual("SUCCESS",changed["status"])
            self.assertNotEqual(result["subject_sha256"],changed["subject_sha256"])

    def test_duplicate_toml_binding_is_not_silently_overwritten(self):
        import tomllib
        with self.assertRaises(tomllib.TOMLDecodeError):
            tomllib.loads('reporter_binding={kind="check-run",app_id=1,app_id=2}')


if __name__ == "__main__":
    unittest.main()
