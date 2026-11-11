#!/usr/bin/env python3
"""Guard: the merge queue must not be enabled while the superseded title
producer is the only `merge_group` title producer.

This is a documentation guard, not a settings mutation. It asserts the
activation checklist exists, that every precondition bullet carries exactly one
stable marker, and that every documented precondition is present. It is a
structural check: deleting a single bullet fails, and a file that merely
contains the key phrases does not pass.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHECKLIST = ROOT / "docs" / "ci" / "merge-queue-activation.md"

MARKER = re.compile(r"<!--precondition:([a-z0-9-]+)-->")
BULLET = re.compile(r"^- \[ \] ")

#: One marker per precondition bullet. Order is documentation order, not a
#: priority. Removing a bullet, or adding one without a marker, must fail.
REQUIRED_PRECONDITIONS = (
    "superseded-producer",
    "ruleset-contexts",
    "exact-h-receipt",
    "shared-producer",
    "singleton-canary",
)

#: The five contexts live main ruleset 16664791 actually requires, all bound to
#: the GitHub Actions App. `Gate Enforcement Contract` is repo policy, not live.
LIVE_REQUIRED_CONTEXTS = (
    "Compile All Targets (bit-rot guard)",
    "Conflict marker check",
    "validate-title",
    "Perl LSP Rust Small Result",
    "ripr+ New Gap Gate",
)


def _read() -> str:
    return CHECKLIST.read_text(encoding="utf-8")


class MergeQueueActivationChecklistTests(unittest.TestCase):
    """Assert the activation checklist is structurally complete."""

    def setUp(self) -> None:
        """Load the checklist once per test."""
        self.assertTrue(
            CHECKLIST.exists(),
            "docs/ci/merge-queue-activation.md is missing; the queue must not be "
            "enabled without the activation checklist",
        )
        self.text = _read()
        self.bullets = [line for line in self.text.splitlines() if BULLET.match(line)]

    def test_queue_identity_is_named(self) -> None:
        """The checklist must name the queue ruleset and its disabled state."""
        for needle in ("17372329", "disabled", "#19257", "#19261", "superseded"):
            with self.subTest(needle=needle):
                self.assertIn(
                    needle, self.text, f"activation checklist must name `{needle}`"
                )

    def test_every_required_precondition_is_marked(self) -> None:
        """Each documented precondition must appear exactly once."""
        found = MARKER.findall(self.text)
        for name in REQUIRED_PRECONDITIONS:
            with self.subTest(precondition=name):
                self.assertEqual(
                    found.count(name),
                    1,
                    f"precondition `{name}` must appear exactly once; found "
                    f"{found.count(name)}",
                )

    def test_every_bullet_carries_exactly_one_marker(self) -> None:
        """A bullet without a marker, or with two, is a structural defect."""
        self.assertGreaterEqual(
            len(self.bullets),
            len(REQUIRED_PRECONDITIONS),
            "the checklist must carry a bullet per precondition",
        )
        for line in self.bullets:
            with self.subTest(bullet=line[:80]):
                self.assertEqual(
                    len(MARKER.findall(line)),
                    1,
                    "every precondition bullet must carry exactly one "
                    "<!--precondition:...--> marker",
                )

    def test_markers_match_the_documented_set(self) -> None:
        """No undocumented precondition marker may be introduced silently."""
        self.assertEqual(
            sorted(MARKER.findall(self.text)),
            sorted(REQUIRED_PRECONDITIONS),
        )

    def test_live_required_contexts_are_named_accurately(self) -> None:
        """The checklist must state the five live contexts, not a wrong set."""
        for context in LIVE_REQUIRED_CONTEXTS:
            with self.subTest(context=context):
                self.assertIn(context, self.text)
        self.assertIn("is repository policy", self.text)
        self.assertIn("not** installed on `16664791`", self.text)

    def test_does_not_claim_the_rulesets_agree(self) -> None:
        """The retired false claim must not come back."""
        lowered = self.text.lower()
        self.assertNotIn("agree on the required contexts", lowered)
        self.assertNotIn("revert #19257 first", lowered)

    def test_receipt_and_singleton_canary_are_required(self) -> None:
        """A real exact-H receipt and a singleton G canary are preconditions."""
        for needle in (
            "exact-H admission receipt",
            "real native G",
            "singleton G",
            "exactly one pull request",
        ):
            with self.subTest(needle=needle):
                self.assertIn(needle, self.text)


if __name__ == "__main__":
    unittest.main()
