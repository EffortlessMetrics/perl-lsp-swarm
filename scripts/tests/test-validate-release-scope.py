#!/usr/bin/env python3
"""Focused tests for the release admission receipt's relational checker."""

from __future__ import annotations

import copy
import importlib.util
import json
import unittest
from pathlib import Path


ROOT = Path(__file__).parents[2]
SPEC = importlib.util.spec_from_file_location("validate_release_scope", ROOT / "scripts/validate-release-scope.py")
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ReleaseScopeValidationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.schema = json.loads((ROOT / "docs/releases/release-scope.schema.json").read_text(encoding="utf-8"))
        cls.receipt = json.loads((ROOT / "docs/releases/v0.18.0-scope.json").read_text(encoding="utf-8"))

    def assert_invalid(self, receipt: dict, message: str) -> None:
        with self.assertRaisesRegex(ValueError, message):
            MODULE.validate_scope(self.schema, receipt)

    def test_current_admission_receipt_passes(self) -> None:
        MODULE.validate_scope(self.schema, self.receipt)

    def test_rejects_duplicate_or_mismatched_queue_identity(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["items"][1]["number"] = receipt["items"][0]["number"]
        receipt["items"][1]["url"] = receipt["items"][0]["url"]
        self.assert_invalid(receipt, "items\\.number")

    def test_rejects_count_or_query_headroom_drift(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["queue_snapshot"]["query_limit"] = receipt["queue_snapshot"]["observed_open_count"] - 1
        self.assert_invalid(receipt, "query_limit")

        receipt["queue_snapshot"]["query_limit"] = 101
        self.assert_invalid(receipt, "pinned --limit 100")

    def test_rejects_non_utc_observation(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["observed_at_utc"] = receipt["observed_at_utc"].replace("Z", "+00:00")
        self.assert_invalid(receipt, "end in Z")

    def test_rejects_post_release_item_without_follow_up(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["items"][0]["follow_up_issue"] = None
        self.assert_invalid(receipt, "follow_up_issue")

    def test_rejects_malformed_follow_up_without_traceback(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["items"][0]["follow_up_issue"] = ["https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1"]
        self.assert_invalid(receipt, "follow_up_issue")

    def test_candidate_requires_release_complete_evidence(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["items"][0]["disposition"] = "0.18-candidate"
        self.assert_invalid(receipt, "0.18-candidate requires owner")


if __name__ == "__main__":
    unittest.main()
