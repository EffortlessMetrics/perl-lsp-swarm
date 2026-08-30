#!/usr/bin/env python3
"""Focused contract and falsifier tests for blocker_closeout.v1."""

from __future__ import annotations

import copy
import importlib.util
import json
import re
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).parents[2]
FIXTURES = ROOT / "scripts" / "tests" / "fixtures" / "blocker_closeout"
SPEC = importlib.util.spec_from_file_location(
    "validate_blocker_closeout", ROOT / "scripts" / "validate_blocker_closeout.py"
)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def _at_path(value: object, path: str) -> object:
    current = value
    for component in path.split("."):
        if isinstance(current, list):
            current = current[int(component)]
        else:
            assert isinstance(current, dict)
            current = current[component]
    return current


def _set_path(value: object, path: str, replacement: object) -> None:
    components = path.split(".")
    parent = _at_path(value, ".".join(components[:-1])) if len(components) > 1 else value
    final = components[-1]
    if isinstance(parent, list):
        parent[int(final)] = replacement
    else:
        assert isinstance(parent, dict)
        parent[final] = replacement


def _apply_case(base: dict, case: dict) -> dict:
    packet = copy.deepcopy(base)
    for path, replacement in case.get("updates", {}).items():
        _set_path(packet, path, copy.deepcopy(replacement))
    for destination, source in case.get("append_from", {}).items():
        target = _at_path(packet, destination)
        assert isinstance(target, list)
        target.append(copy.deepcopy(_at_path(packet, source)))
    return packet


class BlockerCloseoutValidationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.schema = json.loads((ROOT / "schemas" / "blocker_closeout.v1.schema.json").read_text(encoding="utf-8"))
        cls.base = json.loads((FIXTURES / "valid_resolved.json").read_text(encoding="utf-8"))
        cls.cases = json.loads((FIXTURES / "cases.json").read_text(encoding="utf-8"))

    def test_schema_metadata_and_closed_terminal_vocabulary_match_the_model(self) -> None:
        self.assertEqual(self.schema["$schema"], "https://json-schema.org/draft/2020-12/schema")
        self.assertEqual(self.schema["title"], MODULE.SCHEMA_VERSION)
        self.assertEqual(
            set(self.schema["properties"]["status"]["enum"]),
            MODULE.TERMINAL_STATUSES,
        )
        MODULE.validate_blocker_closeout(self.base, lambda _ancestor, _subject: True)

    def test_valid_terminal_states_are_first_class(self) -> None:
        valid_cases = [case for case in self.cases if case["name"].startswith("valid_")]
        self.assertEqual([case["name"] for case in valid_cases], ["valid_bounded_limitation", "valid_blocked", "valid_not_proven"])
        model = MODULE.validate_blocker_closeout(self.base, lambda _ancestor, _subject: True)
        self.assertEqual(model.status, "resolved")
        self.assertEqual(model.controller_evidence.ref, self.base["semantic_controller"]["evidence"]["ref"])
        self.assertEqual(model.review_authority_number, 90002)
        self.assertEqual(model.review_evidence.ref, self.base["review"]["current_head_synthesis"]["ref"])
        for case in valid_cases:
            packet = _apply_case(self.base, case)
            model = MODULE.validate_blocker_closeout(packet, lambda _ancestor, _subject: True)
            self.assertEqual(model.status, case["name"].removeprefix("valid_"))

    def test_blocked_and_not_proven_do_not_claim_closure_proof_coverage(self) -> None:
        cases = {case["name"]: case for case in self.cases}
        for name in ("valid_blocked", "valid_not_proven"):
            with self.subTest(case=name):
                packet = _apply_case(self.base, cases[name])
                passed_claims = {
                    claim_id
                    for observation in packet["proof"]["observations"]
                    if observation["status"] == "passed"
                    for claim_id in observation["claim_ids"]
                }
                self.assertNotIn("example.installed", passed_claims)
                self.assertIn("example.installed", packet["claim_effect"]["preserves"])
                MODULE.validate_blocker_closeout(packet, lambda _ancestor, _subject: True)

        controller_only = _apply_case(self.base, cases["valid_not_proven"])
        controller_only["implementation_prs"] = []
        controller_only["merged_shas"] = []
        controller_only["implementation_contributions"] = []
        controller_only["landed_integrations"] = []
        controller_only["review"].update(
            {
                "authority_kind": "semantic_controller",
                "authority_number": 90001,
                "current_head_synthesis": copy.deepcopy(controller_only["semantic_controller"]["evidence"]),
                "status": "not_proven",
            }
        )
        MODULE.validate_blocker_closeout(controller_only, lambda _ancestor, _subject: True)

    def test_landed_tree_review_can_bind_the_exact_observed_tree(self) -> None:
        packet = copy.deepcopy(self.base)
        packet["review"].update(
            {
                "authority_kind": "landed_tree",
                "authority_number": None,
                "current_head_synthesis": {
                    "kind": "repository_receipt",
                    "ref": f"repo:receipts/landed-tree-review.json@{packet['observed_main_sha']}",
                    "digest": "sha256:abababababababababababababababababababababababababababababababab",
                },
                "reviewed_head": packet["observed_main_sha"],
            }
        )
        model = MODULE.validate_blocker_closeout(packet, lambda _ancestor, _subject: True)
        self.assertEqual(model.review_authority_kind, "landed_tree")
        self.assertIsNone(model.review_authority_number)

    def test_each_fail_closed_rule_has_a_focused_negative_fixture(self) -> None:
        negative_cases = [case for case in self.cases if case["name"].startswith("reject_")]
        self.assertGreaterEqual(len(negative_cases), 38)
        for case in negative_cases:
            with self.subTest(case=case["name"]):
                packet = _apply_case(self.base, case)
                ancestor_result = case.get("ancestor_result", True)
                with self.assertRaisesRegex(ValueError, re.escape(case["error"])):
                    MODULE.validate_blocker_closeout(packet, lambda _ancestor, _subject: ancestor_result)

    def test_reachability_instrument_failure_is_not_proven(self) -> None:
        def failed_instrument(_ancestor: str, _subject: str) -> bool:
            raise RuntimeError("object database unavailable")

        with self.assertRaisesRegex(ValueError, "reachability is not proven"):
            MODULE.validate_blocker_closeout(self.base, failed_instrument)

    def test_shared_implementation_requires_an_explicit_claim_bound_relation(self) -> None:
        packet = copy.deepcopy(self.base)
        packet["shared_implementation_relations"] = [
            {
                "implementation_pr": 90002,
                "other_blocker_id": "other.blocker",
                "relation": "shared",
                "claim_ids": ["example.product"],
                "evidence": {
                    "kind": "github_issue_comment",
                    "ref": "https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/90001#issuecomment-3",
                    "digest": "sha256:8888888888888888888888888888888888888888888888888888888888888888",
                },
            }
        ]
        MODULE.validate_blocker_closeout(packet, lambda _ancestor, _subject: True)
        packet["shared_implementation_relations"][0]["claim_ids"] = ["unknown.claim"]
        with self.assertRaisesRegex(ValueError, "exceed the contribution"):
            MODULE.validate_blocker_closeout(packet, lambda _ancestor, _subject: True)

    def test_contract_cannot_grow_freeze_or_publication_authority(self) -> None:
        for forbidden in ("blocker_denominator", "freeze_ready", "frozen_product_sha", "tag", "publication"):
            with self.subTest(field=forbidden):
                packet = copy.deepcopy(self.base)
                packet[forbidden] = "invented"
                with self.assertRaisesRegex(ValueError, "unexpected field"):
                    MODULE.validate_blocker_closeout(packet, lambda _ancestor, _subject: True)

    def test_cli_checks_real_git_ancestry_and_fails_cleanly(self) -> None:
        self.assertEqual(
            MODULE.main(["--packet", str(FIXTURES / "valid_resolved.json"), "--repository", str(ROOT)]),
            0,
        )
        packet = copy.deepcopy(self.base)
        packet["status"] = "invented"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "invalid.json"
            path.write_text(json.dumps(packet), encoding="utf-8")
            self.assertEqual(MODULE.main(["--packet", str(path), "--repository", str(ROOT)]), 1)


if __name__ == "__main__":
    unittest.main()
