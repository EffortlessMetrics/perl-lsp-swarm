#!/usr/bin/env python3
"""Focused tests for the 0.18.0-rc.1 intake receipt's relational checker."""

from __future__ import annotations

import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).parents[2]
SPEC = importlib.util.spec_from_file_location("validate_rc_intake", ROOT / "scripts/validate-rc-intake.py")
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class RcIntakeValidationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.receipt = json.loads((ROOT / "docs/releases/v0.18.0-rc.1-intake.json").read_text(encoding="utf-8"))

    def assert_invalid(self, receipt: dict, message: str) -> None:
        with self.assertRaisesRegex(ValueError, message):
            MODULE.validate_intake(receipt)

    def test_current_intake_receipt_passes(self) -> None:
        MODULE.validate_intake(self.receipt)
        self.assertEqual(MODULE.main([]), 0)

    def test_rejects_release_affecting_pr_omitted_from_every_disposition(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["excluded_post_rc"] = [
            entry for entry in receipt["excluded_post_rc"] if entry["number"] != 12290
        ]
        self.assert_invalid(receipt, "omitted from every disposition")

    def test_rejects_pr_in_two_dispositions(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        entry = receipt["excluded_post_rc"][0]
        receipt["not_release_relevant"].append(
            {"number": entry["number"], "reason": "duplicate placement must fail"}
        )
        self.assert_invalid(receipt, "overlaps an earlier disposition")

    def test_rejects_disposition_outside_observed_queue(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["not_release_relevant"].append(
            {"number": 999999, "reason": "not observed at observation_sha"}
        )
        self.assert_invalid(receipt, "outside the observed open queue")

    def test_rejects_feature_intake_open(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["feature_intake_closed"] = False
        self.assert_invalid(receipt, "feature_intake_closed must be true")

    def test_rejects_unbounded_allowed_change_class(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["allowed_change_classes"] = ["rc-reproduced-blocker", "anything-else"]
        self.assert_invalid(receipt, "bounded release class list")

    def test_rejects_required_blocker_without_owner_bounded_repair_and_proof(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        moved = next(
            entry
            for entry in receipt["not_release_relevant"]
            if entry["number"] == 12320
        )
        receipt["not_release_relevant"] = [
            entry for entry in receipt["not_release_relevant"] if entry["number"] != 12320
        ]
        receipt["required_blockers"] = [{"number": moved["number"]}]
        self.assert_invalid(receipt, "required_blockers\\[0\\]")

    def test_rejects_not_proven_represented_as_included(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["not_proven"] = [{"number": 12320, "reason": "relevance could not be determined"}]
        receipt["included_prs"] = [
            {"number": 12320, "reason": "unproven work smuggled into the RC"}
        ]
        self.assert_invalid(receipt, "overlaps an earlier disposition")

    def test_rejects_already_included_entry_with_candidate_head_shape(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["already_included"][0]["landed_sha"] = "short-head-ref"
        self.assert_invalid(receipt, "already_included\\[0\\].landed_sha")

    def test_rejects_non_null_frozen_product_sha_at_admission(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["frozen_product_sha"] = "a" * 40
        self.assert_invalid(receipt, "cannot record a frozen product SHA")

    def test_rejects_drifted_public_claim_boundary(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["public_claim_boundary"]["formatting"] = "range-enabled"
        self.assert_invalid(receipt, "public_claim_boundary must match")

    def test_rejects_issue_closure_relations(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["issue_closures_required"] = [5888]
        self.assert_invalid(receipt, "issue_closures_required must remain empty")

    def test_rejects_non_canonical_bytes(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        with tempfile.TemporaryDirectory() as directory:
            receipt_path = Path(directory) / "receipt.json"
            receipt_path.write_text(json.dumps(receipt), encoding="utf-8")
            self.assertEqual(MODULE.main(["--receipt", str(receipt_path)]), 1)

    def test_canonical_round_trip_is_stable(self) -> None:
        first = MODULE.canonical_bytes(self.receipt)
        second = MODULE.canonical_bytes(json.loads(first))
        self.assertEqual(first, second)


if __name__ == "__main__":
    unittest.main()
