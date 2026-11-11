#!/usr/bin/env python3
"""Guard: the merge queue must not be enabled while the superseded title
producer is the only `merge_group` title producer.

This is a documentation guard, not a settings mutation. It asserts the
activation checklist exists and names the superseded-producer precondition so
a fresh session cannot enable queue ruleset 17372329 without seeing it.
"""

from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHECKLIST = ROOT / "docs" / "ci" / "merge-queue-activation.md"


class MergeQueueActivationChecklistTests(unittest.TestCase):
    def test_checklist_exists(self) -> None:
        self.assertTrue(
            CHECKLIST.exists(),
            "docs/ci/merge-queue-activation.md is missing; the queue must not be "
            "enabled without the activation checklist",
        )

    def test_checklist_names_the_superseded_producer(self) -> None:
        text = CHECKLIST.read_text(encoding="utf-8")
        for needle in (
            "17372329",
            "#19257",
            "#19261",
            "commit-to-PR association",
            "superseded",
        ):
            self.assertIn(needle, text, f"activation checklist must name `{needle}`")

    def test_checklist_requires_a_real_g_receipt(self) -> None:
        text = CHECKLIST.read_text(encoding="utf-8")
        self.assertIn("exact-H admission receipt", text)
        self.assertIn("real native G", text)


if __name__ == "__main__":
    unittest.main()
