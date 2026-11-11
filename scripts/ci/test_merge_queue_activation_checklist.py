#!/usr/bin/env python3
"""Guard: the merge queue must not be enabled while the superseded title
producer is the only `merge_group` title producer.

This is a documentation guard, not a settings mutation. It asserts the
activation checklist exists, that its precondition bullets sit under
`## Preconditions`, that each carries exactly one stable marker and its
load-bearing phrases, and that the queue is described as disabled. It is a
structural check: deleting a bullet, moving one, or gutting its prose fails,
and a file that merely contains the key phrases does not pass.

Scope boundary: this guard reads one file. It does not query the live rulesets,
does not verify that the prose is true, and does not enable or disable the
queue. Those remain steward actions with their own read-back.
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

#: Phrases each precondition bullet must still carry, so that gutting the prose
#: while leaving the marker behind fails. Kept deliberately small: one or two
#: load-bearing phrases per bullet, not the whole sentence.
PRECONDITION_TEXT = {
    "superseded-producer": ("commit-to-PR association", "#19261"),
    "ruleset-contexts": ("required_status_checks", "App-owned aggregate"),
    "exact-h-receipt": ("exact-H admission receipt",),
    "shared-producer": ("required-merge-surface commands", "NOT_PROVEN"),
    "singleton-canary": ("exactly one pull request", "merge_group"),
}

#: The heading the precondition bullets must live under.
PRECONDITIONS_HEADING = "Preconditions"

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


def _context_list() -> str:
    """The exact enumeration the checklist must carry, in order."""
    return ", ".join(f"`{context}`" for context in LIVE_REQUIRED_CONTEXTS)


def _sections(text: str) -> dict[str, str]:
    """Map each `## Heading` to its body, so a check can be scoped."""
    sections: dict[str, list[str]] = {}
    heading: str | None = None
    for line in text.splitlines():
        if line.startswith("## "):
            heading = line[3:].strip()
            sections[heading] = []
        elif heading is not None:
            sections[heading].append(line)
    return {name: "\n".join(body) for name, body in sections.items()}



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
        self.sections = _sections(self.text)
        self.bullets = [line for line in self.text.splitlines() if BULLET.match(line)]

    def test_preconditions_heading_is_required(self) -> None:
        """The bullets must live under one `## Preconditions` heading."""
        self.assertEqual(
            [line for line in self.text.splitlines() if line.strip() == "## Preconditions"],
            ["## Preconditions"],
            "the checklist must carry exactly one `## Preconditions` heading",
        )
        self.assertIn(PRECONDITIONS_HEADING, self.sections)

    def test_preconditions_live_under_their_heading(self) -> None:
        """A precondition moved into another section is not a precondition."""
        body = self.sections[PRECONDITIONS_HEADING]
        for name in REQUIRED_PRECONDITIONS:
            with self.subTest(precondition=name):
                self.assertIn(
                    f"<!--precondition:{name}-->",
                    body,
                    f"precondition `{name}` must sit under `## Preconditions`",
                )
        outside = [
            name
            for heading, text in self.sections.items()
            if heading != PRECONDITIONS_HEADING
            for name in MARKER.findall(text)
        ]
        self.assertEqual(
            outside, [], "no precondition marker may appear outside `## Preconditions`"
        )

    def test_each_bullet_keeps_its_load_bearing_phrases(self) -> None:
        """Leaving a marker behind while gutting its prose must fail."""
        bullets = {
            name: line
            for line in self.bullets
            for name in MARKER.findall(line)
        }
        for name, phrases in PRECONDITION_TEXT.items():
            with self.subTest(precondition=name):
                self.assertIn(name, bullets, f"missing bullet for `{name}`")
                for phrase in phrases:
                    self.assertIn(
                        phrase,
                        bullets[name],
                        f"precondition `{name}` must still state `{phrase}`",
                    )

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
        # The list itself must be exact: a renamed, added or dropped context
        # inside the enumeration has to fail, not just a missing name.
        self.assertEqual(
            self.text.count(_context_list()),
            1,
            "the checklist must enumerate exactly the five live required "
            "contexts, in order",
        )
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

    def test_queue_state_is_stated_as_disabled(self) -> None:
        """The queue must be described as disabled, in both places, literally.

        A prose search for the word `disabled` is not enough: it also appears in
        the reversion section, so inverting either statement would still pass.
        """
        self.assertEqual(
            self.text.count("must stay **disabled**"),
            1,
            "the checklist must state that the queue must stay **disabled**",
        )
        self.assertEqual(
            self.text.count("is **disabled**"),
            1,
            "the checklist must state that ruleset 17372329 is **disabled**",
        )
        self.assertNotIn(
            "**enabled**",
            self.text,
            "the checklist must not describe the queue as enabled",
        )

    def test_reversion_section_records_the_steward_decision(self) -> None:
        """The reversion policy must stay present and must not invite a revert."""
        sections = _sections(self.text)
        self.assertIn(
            "Reverting #19257",
            sections,
            "the checklist must keep a `## Reverting #19257` section",
        )
        reversion = sections["Reverting #19257"]
        for needle in ("steward decision", "no revert", "keep `1ec099411` on main"):
            with self.subTest(needle=needle):
                self.assertIn(
                    needle,
                    reversion,
                    f"the reversion section must state `{needle}`",
                )

    def test_guard_section_is_present(self) -> None:
        """The guard section must describe what the test actually enforces."""
        self.assertIn("Guard", _sections(self.text))


if __name__ == "__main__":
    unittest.main()
