#!/usr/bin/env python3
"""Offline changed-path and execution controls for the review helper lane (#18154)."""

from pathlib import Path
import re
import unittest

from parser_facade_authority import (
    pull_request_path_pattern_matches,
    read_pull_request_path_filters,
)

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/ci-gate-self-tests.yml"
SELF = "scripts/ci/test_ci_gate_self_test_routing.py"
FEEDBACK_TEST = "scripts/tests/test_review_feedback.py"


class ReviewHelperRouting(unittest.TestCase):
    def selected(self, changed_paths):
        filters = read_pull_request_path_filters(WORKFLOW)
        return any(
            pull_request_path_pattern_matches(pattern, path)
            for path in changed_paths
            for pattern in filters
        )

    def test_sources_and_test_only_changes_select_workflow(self):
        for path in (
            "scripts/reviews/threads",  # Bash source without a suffix
            "scripts/reviews/inline",
            "scripts/reviews/feedback",  # Python source without a suffix
            "scripts/tests/test-review-threads-inline.sh",
            FEEDBACK_TEST,
            SELF,
        ):
            with self.subTest(changed_path=path):
                self.assertTrue(self.selected([path]), f"unrouted: {path}")

    def test_unrelated_changes_remain_unselected(self):
        unrelated = [
            "crates/perl-parser/src/parser.rs",
            "docs/how-to/unrelated.md",
            "scripts/tests/test-unrelated.sh",
            "scripts/ci/unrelated.py",
            "scripts/reviews-backup/feedback",
        ]
        for path in unrelated:
            with self.subTest(changed_path=path):
                self.assertFalse(self.selected([path]), f"overbroad route: {path}")
        self.assertFalse(self.selected([]))
        self.assertFalse(self.selected(unrelated))
        self.assertTrue(self.selected([*unrelated, FEEDBACK_TEST]))

    def test_existing_job_executes_shell_python_and_routing_suites(self):
        text = WORKFLOW.read_text(encoding="utf-8")
        job = re.search(
            r"(?ms)^  review-threads-inline-self-test:\n(.*?)(?=^  \S|\Z)", text
        )
        self.assertIsNotNone(job, "existing review helper job is missing")
        body = job.group(1)
        self.assertNotRegex(body, r"(?m)^    (?:if|continue-on-error):")
        steps = re.findall(
            r"(?m)^      - name: [^\n]+\n((?:        [^\n]*\n|\n)*)", body
        )
        for command in (
            "bash scripts/tests/test-review-threads-inline.sh",
            f"python3 {FEEDBACK_TEST} -v",
            f"python3 {SELF} -v",
        ):
            with self.subTest(command=command):
                self.assertIn(
                    f"run: {command}", [step.strip() for step in steps],
                    "suite must execute unconditionally with its exit status preserved",
                )


if __name__ == "__main__":
    unittest.main()
