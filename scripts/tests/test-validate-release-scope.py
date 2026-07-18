#!/usr/bin/env python3
"""Focused tests for the release admission receipt's relational checker."""

from __future__ import annotations

import copy
import importlib.util
import json
import tempfile
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
        receipt["queue_snapshot"]["observed_open_count"] = 100
        self.assert_invalid(receipt, "headroom")

        receipt["queue_snapshot"]["query_limit"] = 101
        self.assert_invalid(receipt, "expected const 100")

    def test_rejects_non_utc_observation(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["observed_at_utc"] = receipt["observed_at_utc"].replace("Z", "+00:00")
        self.assert_invalid(receipt, "does not match pattern")

    def test_rejects_post_release_item_without_follow_up(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["items"][0]["follow_up_issue"] = None
        self.assert_invalid(receipt, "follow_up_issue")

    def test_rejects_malformed_follow_up_without_traceback(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt["items"][0]["follow_up_issue"] = ["https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1"]
        self.assert_invalid(receipt, "follow_up_issue")

    def test_rejects_missing_schema_required_field(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        receipt.pop("proof_refs")
        self.assert_invalid(receipt, "missing required property 'proof_refs'")

        with tempfile.TemporaryDirectory() as directory:
            directory_path = Path(directory)
            schema_path = directory_path / "schema.json"
            receipt_path = directory_path / "receipt.json"
            schema_path.write_text(json.dumps(self.schema), encoding="utf-8")
            receipt_path.write_text(json.dumps(receipt), encoding="utf-8")
            self.assertEqual(MODULE.main(["--schema", str(schema_path), "--receipt", str(receipt_path)]), 1)

    def test_candidate_requires_release_complete_evidence(self) -> None:
        receipt = copy.deepcopy(self.receipt)
        candidate = receipt["items"][0]
        candidate["disposition"] = "0.18-candidate"
        candidate["owner"] = "@steven"
        candidate["acceptance"] = ["candidate acceptance"]
        candidate["proof"] = ["candidate proof"]
        candidate["unresolved_threads"] = 0
        candidate["checks"]["failed"] = 0
        candidate["checks"]["pending"] = 0
        MODULE.validate_scope(self.schema, receipt)

        for field, value, message in (
            ("unresolved_threads", 1, "expected const 0"),
            ("checks.failed", 1, "expected const 0"),
            ("checks.pending", 1, "expected const 0"),
        ):
            invalid = copy.deepcopy(receipt)
            if "." in field:
                object_name, nested_field = field.split(".")
                invalid["items"][0][object_name][nested_field] = value
            else:
                invalid["items"][0][field] = value
            self.assert_invalid(invalid, message)


if __name__ == "__main__":
    unittest.main()
