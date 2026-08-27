#!/usr/bin/env python3
"""Non-Cargo regression proof for the release-doc synchronizer anchor."""

from __future__ import annotations

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).parents[2]
SYNCHRONIZER = ROOT / "xtask/src/tasks/sync_release_docs.rs"
RELEASE_STATUS = ROOT / "docs/project/status/release.md"

# These are the two stable narrative forms accepted by sync_release_notes().
# Keep the check intentionally independent of Cargo so the documentation contract
# is falsifiable before the xtask binary is rebuilt.
ANCHOR = re.compile(
    r"^- Remaining work is operational: "
    r"(?:finish|verify the existing) `v[^`]+` "
    r"(?:prep verification, then publish and record final channel receipts"
    r"|release receipt and close the remaining channel receipts).*"
)


def synchronizer_anchors(text: str) -> list[str]:
    return [line for line in text.splitlines() if ANCHOR.fullmatch(line)]


class ReleaseDocsSynchronizerAnchorTests(unittest.TestCase):
    def test_candidate_preserves_a_recognized_anchor(self) -> None:
        source = SYNCHRONIZER.read_text(encoding="utf-8")
        docs = RELEASE_STATUS.read_text(encoding="utf-8")

        self.assertIn(
            'line.starts_with("- Remaining work is operational: finish `v")',
            source,
        )
        self.assertIn(
            'line.starts_with("- Remaining work is operational: verify the existing `v")',
            source,
        )
        self.assertIn("remaining_seen = true", source)
        self.assertTrue(
            synchronizer_anchors(docs),
            "release.md must retain a line recognized by sync_release_notes",
        )

    def test_missing_anchor_is_a_real_regression(self) -> None:
        docs = RELEASE_STATUS.read_text(encoding="utf-8")
        without_anchor = "\n".join(
            line for line in docs.splitlines() if not ANCHOR.fullmatch(line)
        )

        self.assertFalse(
            synchronizer_anchors(without_anchor),
            "the negative control must model the pre-repair missing-anchor state",
        )

    def test_anchor_does_not_promote_release_state(self) -> None:
        docs = RELEASE_STATUS.read_text(encoding="utf-8")
        anchor = synchronizer_anchors(docs)

        self.assertEqual(len(anchor), 1)
        self.assertIn("do not dispatch release orchestration", anchor[0])
        self.assertNotIn("publishable", anchor[0])


if __name__ == "__main__":
    unittest.main()
