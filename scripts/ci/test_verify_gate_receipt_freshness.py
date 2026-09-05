#!/usr/bin/env python3
"""Falsifiers for verify_gate_receipt_freshness.py."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("verify_gate_receipt_freshness.py")
SPEC = importlib.util.spec_from_file_location("verify_gate_receipt_freshness", SCRIPT)
assert SPEC and SPEC.loader
verifier = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = verifier
SPEC.loader.exec_module(verifier)
SUBJECT = "a" * 40


def gate_receipt(gate: str, *, sha: str = SUBJECT) -> dict[str, object]:
    return {
        "schema_version": "1.0.0",
        "metadata": {"git_sha": sha},
        "gates": [{"gate_name": gate}],
    }


def shard_summary(*, sha: str = SUBJECT) -> dict[str, object]:
    return {
        "schema_version": "ci_gate_shard.v1",
        "subject_sha": sha,
        "gates": [],
    }


class VerifyGateReceiptFreshnessTests(unittest.TestCase):
    def test_fresh_gate_receipt_and_summary_pass(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            receipts = root / "shards"
            summaries = root / "summaries"
            receipts.mkdir()
            summaries.mkdir()
            (receipts / "alpha.json").write_text(
                json.dumps(gate_receipt("alpha")), encoding="utf-8"
            )
            (summaries / "meta.json").write_text(
                json.dumps(shard_summary()), encoding="utf-8"
            )
            self.assertEqual(
                [],
                verifier.find_stale_artifacts([receipts, summaries], SUBJECT),
            )
            self.assertEqual(
                0,
                verifier.main(
                    [
                        "--subject-sha",
                        SUBJECT,
                        "--receipt-dir",
                        str(receipts),
                        "--summary-dir",
                        str(summaries),
                    ]
                ),
            )

    def test_missing_directories_offer_nothing_and_pass(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            absent = Path(tmp) / "never-created"
            self.assertEqual(
                [], verifier.find_stale_artifacts([absent], SUBJECT)
            )
            self.assertEqual(
                0,
                verifier.main(
                    [
                        "--subject-sha",
                        SUBJECT,
                        "--receipt-dir",
                        str(absent),
                        "--summary-dir",
                        str(absent),
                    ]
                ),
            )

    def test_foreign_subject_bindings_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            receipts = root / "shards"
            summaries = root / "summaries"
            receipts.mkdir()
            summaries.mkdir()
            stale_receipt = receipts / "alpha.json"
            stale_receipt.write_text(
                json.dumps(gate_receipt("alpha", sha="b" * 40)),
                encoding="utf-8",
            )
            stale_summary = summaries / "meta.json"
            stale_summary.write_text(
                json.dumps(shard_summary(sha="c" * 40)), encoding="utf-8"
            )
            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                status = verifier.main(
                    [
                        "--subject-sha",
                        SUBJECT,
                        "--receipt-dir",
                        str(receipts),
                        "--summary-dir",
                        str(summaries),
                    ]
                )
        self.assertEqual(1, status)
        message = stderr.getvalue()
        self.assertIn(str(stale_receipt), message)
        self.assertIn(str(stale_summary), message)
        self.assertIn("expected", message)

    def test_unbindable_or_corrupt_entries_fail_closed(self) -> None:
        cases = {
            "malformed.json": "{not json",
            "unbound_summary.json": json.dumps({"schema_version": "other"}),
            "unbound_gate_receipt.json": json.dumps({"metadata": {}}),
            "non_object_root.json": json.dumps([1, 2]),
        }
        for name, content in cases.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as tmp:
                receipts = Path(tmp) / "shards"
                receipts.mkdir()
                (receipts / name).write_text(content, encoding="utf-8")
                offenders = verifier.find_stale_artifacts([receipts], SUBJECT)
                self.assertEqual(1, len(offenders))
                self.assertIn(name, offenders[0])

    def test_directory_named_json_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipts = Path(tmp) / "shards"
            (receipts / "sneaky.json").mkdir(parents=True)
            offenders = verifier.find_stale_artifacts([receipts], SUBJECT)
        self.assertEqual(1, len(offenders))
        self.assertIn("not a regular non-symlink file", offenders[0])

    def test_empty_binding_never_matches_a_subject(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            summaries = Path(tmp) / "summaries"
            summaries.mkdir()
            (summaries / "empty.json").write_text(
                json.dumps(
                    {"schema_version": "ci_gate_shard.v1", "subject_sha": ""}
                ),
                encoding="utf-8",
            )
            offenders = verifier.find_stale_artifacts([summaries], SUBJECT)
        self.assertEqual(1, len(offenders))
        self.assertIn("binds subject ''", offenders[0])
        self.assertIsNone(verifier.bound_subject({"metadata": {"git_sha": 7}}))


if __name__ == "__main__":
    unittest.main()
