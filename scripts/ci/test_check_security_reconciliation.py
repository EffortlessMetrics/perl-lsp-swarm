#!/usr/bin/env python3
from __future__ import annotations

import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("check_security_reconciliation.py")
SPEC = importlib.util.spec_from_file_location("check_security_reconciliation", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

LEDGER = Path(__file__).resolve().parents[2] / ".ci/security/may-2026-findings.json"
MARKDOWN = Path(__file__).resolve().parents[2] / "docs/security/may-2026-findings.md"


class SecurityReconciliationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.valid = MODULE.load_ledger(LEDGER)

    def assert_invalid(self, data: dict, needle: str) -> None:
        with self.assertRaisesRegex(MODULE.LedgerError, needle):
            MODULE.validate_ledger(data)

    def test_checked_in_ledger_and_markdown_are_current(self) -> None:
        MODULE.validate_ledger(self.valid)
        self.assertEqual(MODULE.render_markdown(self.valid), MARKDOWN.read_text(encoding="utf-8"))

    def test_missing_original_row_fails(self) -> None:
        data = copy.deepcopy(self.valid)
        data["findings"].pop()
        self.assert_invalid(data, "exactly 60")

    def test_duplicate_source_identity_fails_even_with_new_id(self) -> None:
        data = copy.deepcopy(self.valid)
        duplicate = copy.deepcopy(data["findings"][0])
        duplicate["id"] = "MAY2026-HIGH-999"
        data["findings"][1] = duplicate
        self.assert_invalid(data, "duplicate source finding identity")

    def test_source_digest_drift_fails(self) -> None:
        data = copy.deepcopy(self.valid)
        data["findings"][0]["title"] += " changed"
        self.assert_invalid(data, "source_text_digest is stale")

    def test_open_pr_cannot_be_recorded_as_merged(self) -> None:
        data = copy.deepcopy(self.valid)
        row = data["findings"][0]
        row["github"]["pr_relationship"] = "merged"
        row["github"]["pr_state"] = "open"
        self.assert_invalid(data, "merged relationship cannot be inferred")

    def test_closed_issue_does_not_create_closure(self) -> None:
        data = copy.deepcopy(self.valid)
        row = data["findings"][0]
        row["github"]["issue_state"] = "closed"
        row["verdict"] = "open"
        MODULE.validate_ledger(data)

    def test_proven_closed_requires_landed_current_source_proof(self) -> None:
        data = copy.deepcopy(self.valid)
        row = data["findings"][0]
        row["verdict"] = "proven_closed"
        row["residual_owner"] = None
        self.assert_invalid(data, "proven_closed requires merged canonical PR")

    def test_false_or_stale_premise_requires_correction(self) -> None:
        data = copy.deepcopy(self.valid)
        row = data["findings"][0]
        row["verdict"] = "false_or_stale_premise"
        row["current_reachability_correction"] = None
        self.assert_invalid(data, "requires an explicit current correction")

    def test_write_then_check_round_trip_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            ledger = root / "ledger.json"
            markdown = root / "ledger.md"
            ledger.write_text(json.dumps(self.valid, indent=2) + "\n", encoding="utf-8")
            MODULE.check_or_write(ledger, markdown, True)
            first = markdown.read_bytes()
            MODULE.check_or_write(ledger, markdown, True)
            self.assertEqual(first, markdown.read_bytes())
            MODULE.check_or_write(ledger, markdown, False)


if __name__ == "__main__":
    unittest.main()
