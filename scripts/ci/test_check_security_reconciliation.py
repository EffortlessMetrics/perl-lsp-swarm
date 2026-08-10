#!/usr/bin/env python3
from __future__ import annotations

import copy
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import security_reconciliation_io as IO
import security_reconciliation_model as MODEL

LEDGER = Path(__file__).resolve().parents[2] / ".ci/security/may-2026-findings.json"
MARKDOWN = Path(__file__).resolve().parents[2] / "docs/security/may-2026-findings.md"


class SecurityReconciliationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.valid = IO.load_ledger(LEDGER)

    def assert_invalid(self, data: dict, needle: str) -> None:
        with self.assertRaisesRegex(MODEL.LedgerError, needle):
            MODEL.validate_ledger(data)

    def test_checked_in_ledger_and_markdown_are_current(self) -> None:
        MODEL.validate_ledger(self.valid)
        self.assertEqual(IO.render_markdown(self.valid), MARKDOWN.read_text(encoding="utf-8"))

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
        MODEL.validate_ledger(data)

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

    def test_finding_files_must_stay_under_may_2026_findings(self) -> None:
        data = json.loads(LEDGER.read_text(encoding="utf-8"))
        data["finding_files"] = ["other.json"]
        with tempfile.TemporaryDirectory() as raw:
            ledger = Path(raw) / "ledger.json"
            ledger.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(MODEL.LedgerError, "may-2026-findings"):
                IO.load_ledger(ledger)

    def test_finding_files_reject_parent_directory_segments(self) -> None:
        data = json.loads(LEDGER.read_text(encoding="utf-8"))
        data["finding_files"] = ["may-2026-findings/../may-2026-findings/high-1.json"]
        with tempfile.TemporaryDirectory() as raw:
            ledger = Path(raw) / "ledger.json"
            ledger.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(MODEL.LedgerError, "parent-directory segments"):
                IO.load_ledger(ledger)

    def test_write_then_check_round_trip_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            ledger = root / "ledger.json"
            markdown = root / "ledger.md"
            source_manifest = json.loads(LEDGER.read_text(encoding="utf-8"))
            ledger.write_text(json.dumps(source_manifest, indent=2) + "\n", encoding="utf-8")
            for relative in source_manifest["finding_files"]:
                source = LEDGER.parent / relative
                target = ledger.parent / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)
            IO.check_or_write(ledger, markdown, True)
            first = markdown.read_bytes()
            IO.check_or_write(ledger, markdown, True)
            self.assertEqual(first, markdown.read_bytes())
            IO.check_or_write(ledger, markdown, False)


if __name__ == "__main__":
    unittest.main()
