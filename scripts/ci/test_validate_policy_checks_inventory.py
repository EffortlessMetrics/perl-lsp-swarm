#!/usr/bin/env python3
"""Falsifiers for the policy_checks inventory validator."""

from __future__ import annotations

import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("validate_policy_checks_inventory.py")
SPEC = importlib.util.spec_from_file_location("policy_checks_inventory", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
validator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(validator)

ROOT = Path(__file__).resolve().parents[2]
GATE_POLICY = ROOT / ".ci/gate-policy.yaml"
INVENTORY = ROOT / ".ci/policy-checks-inventory.json"
DOC = ROOT / "docs/ci/policy-checks-inventory.md"


class PolicyChecksInventoryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.source = validator.extract_policy_checks(
            GATE_POLICY.read_text(encoding="utf-8")
        )
        self.inventory = json.loads(INVENTORY.read_text(encoding="utf-8"))

    def errors(self, inventory: dict) -> list[str]:
        return validator.validate_inventory(inventory, self.source)

    def test_current_inventory_and_projection_are_exact(self) -> None:
        self.assertEqual(
            validator.check_paths(GATE_POLICY, INVENTORY, DOC),
            [],
        )

    def test_omitted_current_member_is_rejected(self) -> None:
        candidate = copy.deepcopy(self.inventory)
        candidate["members"].pop(3)
        errors = self.errors(candidate)
        self.assertTrue(
            any("member count" in error or "source order" in error for error in errors),
            errors,
        )

    def test_duplicate_stable_id_is_rejected(self) -> None:
        candidate = copy.deepcopy(self.inventory)
        candidate["members"][1]["stable_id"] = candidate["members"][0]["stable_id"]
        errors = self.errors(candidate)
        self.assertTrue(any("duplicate stable_id" in error for error in errors), errors)

    def test_ownerless_or_claimless_member_is_rejected(self) -> None:
        candidate = copy.deepcopy(self.inventory)
        candidate["members"][0]["owner"] = ""
        candidate["members"][1]["claim"] = ""
        errors = self.errors(candidate)
        self.assertTrue(any(".owner" in error for error in errors), errors)
        self.assertTrue(any(".claim" in error for error in errors), errors)

    def test_reordering_members_is_not_semantically_neutral(self) -> None:
        candidate = copy.deepcopy(self.inventory)
        candidate["members"][0], candidate["members"][1] = (
            candidate["members"][1],
            candidate["members"][0],
        )
        errors = self.errors(candidate)
        self.assertTrue(any(".position" in error for error in errors), errors)
        self.assertTrue(any("source order" in error for error in errors), errors)

    def test_stale_historical_member_is_rejected(self) -> None:
        candidate = copy.deepcopy(self.inventory)
        candidate["members"].append(
            {
                **copy.deepcopy(candidate["members"][-1]),
                "position": len(candidate["members"]) + 1,
                "stable_id": "historical_removed_member",
                "command": "cargo xtask historical-removed-member",
            }
        )
        errors = self.errors(candidate)
        self.assertTrue(any("member count" in error for error in errors), errors)

    def test_external_authority_requires_named_target(self) -> None:
        candidate = copy.deepcopy(self.inventory)
        candidate["members"][7]["overlap"]["targets"] = []
        errors = self.errors(candidate)
        self.assertTrue(
            any("targets is required for authoritative_elsewhere" in error for error in errors),
            errors,
        )

    def test_source_fingerprint_is_load_bearing(self) -> None:
        candidate = copy.deepcopy(self.inventory)
        candidate["source"]["command_fingerprint_sha256"] = "0" * 64
        errors = self.errors(candidate)
        self.assertTrue(any("command_fingerprint_sha256" in error for error in errors), errors)

    def test_generated_projection_drift_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            policy = root / "gate-policy.yaml"
            inventory = root / "inventory.json"
            doc = root / "projection.md"
            policy.write_text(GATE_POLICY.read_text(encoding="utf-8"), encoding="utf-8")
            inventory.write_text(INVENTORY.read_text(encoding="utf-8"), encoding="utf-8")
            doc.write_text("stale\n", encoding="utf-8")
            errors = validator.check_paths(policy, inventory, doc)
            self.assertTrue(any("projection drift" in error for error in errors), errors)


if __name__ == "__main__":
    unittest.main()
