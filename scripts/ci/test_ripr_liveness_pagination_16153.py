#!/usr/bin/env python3
"""Workflow contract for the multi-page ripr-liveness snapshot (issue #16153).

The classifier in ``ripr_liveness.py`` decides whether a queued ripr run is an
explained wait (``serialised_behind_predecessor``) or an absence of proof
(``infra-no-proof``). The decision is made against whatever runs the
``Snapshot ripr runs`` step put into ``target/ripr/liveness/runs.json``, so the
fidelity of that snapshot is part of the classifier's contract: a
long-running predecessor that falls off page one is invisible to
``predecessor_for``, and an explained wait would then be mis-reported as an
infra alarm.

This contract pins the workflow shape that keeps a worst-case predecessor
visible inside the snapshot, and asserts a negative control that captures the
defect class.
"""

from __future__ import annotations

import os
import re
import unittest
from pathlib import Path

REPO_ROOT = Path(os.environ.get("A3_REPO_ROOT", Path(__file__).resolve().parents[2]))
WORKFLOW = REPO_ROOT / ".github/workflows/ripr-liveness.yml"
SELF_TEST_WORKFLOW = REPO_ROOT / ".github/workflows/ci-gate-self-tests.yml"


def lines() -> list[str]:
    if not WORKFLOW.is_file():
        raise FileNotFoundError(WORKFLOW)
    return WORKFLOW.read_text(encoding="utf-8").splitlines()


def self_test_lines() -> list[str]:
    if not SELF_TEST_WORKFLOW.is_file():
        raise FileNotFoundError(SELF_TEST_WORKFLOW)
    return SELF_TEST_WORKFLOW.read_text(encoding="utf-8").splitlines()


def block(source: list[str], marker: str, indent: int) -> list[str]:
    prefix = " " * indent + marker + ":"
    starts = [i for i, line in enumerate(source) if line == prefix]
    if len(starts) != 1:
        raise AssertionError(f"expected one {marker!r}, found {len(starts)}")
    start = starts[0]
    end = next(
        (i for i in range(start + 1, len(source))
         if source[i] and not source[i].startswith(" " * (indent + 1))),
        len(source),
    )
    return source[start:end]


def find_run_blocks(source: list[str]) -> list[str]:
    """Every ``run: |`` heredoc body under the Snapshot ripr runs step.

    YAML block scalars indent the body two spaces past the key, so the body
    sits at indent 10 here. We keep every line whose stripped indent is at
    least 10 and stop when we encounter a line that closes the block (the
    next step's ``- name:``).
    """

    step_index = next(
        (i for i, line in enumerate(source) if line.strip() == "- name: Snapshot ripr runs"),
        None,
    )
    if step_index is None:
        raise AssertionError("Snapshot ripr runs step is missing")
    run_index = next(
        (i for i in range(step_index, len(source))
         if source[i].lstrip().startswith("run:")),
        None,
    )
    if run_index is None:
        raise AssertionError("Snapshot ripr runs step has no run script")
    body_indent = len(source[run_index + 1]) - len(source[run_index + 1].lstrip())
    block_lines: list[str] = []
    for line in source[run_index + 1:]:
        if not line.strip():
            block_lines.append("")
            continue
        indent = len(line) - len(line.lstrip())
        if indent < body_indent:
            break
        block_lines.append(line[body_indent:])
    while block_lines and block_lines[-1] == "":
        block_lines.pop()
    return block_lines


class RiprLivenessPaginationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.source = lines()
        cls.run_script = "\n".join(find_run_blocks(cls.source))

    def test_the_snapshots_step_fetches_more_than_one_page(self) -> None:
        """A single page caps at 100 runs; the predecessor can fall off it."""
        page_calls = re.findall(
            r"gh api[^\n]*?workflows/ripr\.yml/runs\?[^\n]*",
            self.run_script,
        )
        self.assertGreaterEqual(
            len(page_calls), 2,
            "the snapshot step must request at least two pages so a worst-case "
            "predecessor (queue + ~31 min analysis) is still scanned inside "
            "the snapshot",
        )
        for call in page_calls:
            self.assertIn("per_page=100", call, f"missing per_page=100: {call}")

    def test_pages_are_merged_into_a_single_deduped_array(self) -> None:
        """``predecessor_for`` sees the union, not page 1 alone."""
        self.assertIn("jq -s", self.run_script, "pages must be joined via jq -s")
        self.assertIn("unique_by(.id)", self.run_script,
                      "merging must dedupe by run id")
        self.assertIn("sort_by(.created_at)", self.run_script,
                      "dedupe by id alone does not give a stable ordering")

    def test_the_snapshot_is_bounded(self) -> None:
        """A noisy history cannot stall the cron."""
        bound = re.search(r"\.\[0:(\d+)\]", self.run_script)
        self.assertIsNotNone(
            bound,
            "the snapshot must bound its final size to keep the cron quick",
        )
        self.assertGreaterEqual(
            int(bound.group(1)), 100,
            "the bound must be at least one page, otherwise we are back to "
            "the one-page defect",
        )
        self.assertLessEqual(
            int(bound.group(1)), 1000,
            "the bound must stay modest; huge snapshots defeat the queueing "
            "argument that motivates this workflow",
        )

    def test_the_workflow_publishes_via_an_existing_check_run(self) -> None:
        """Posting lives in a later step, so the snapshot change cannot regress it."""
        post_step = next(
            (i for i, line in enumerate(self.source)
             if line.strip() == "- name: Post advisory check runs"),
            None,
        )
        self.assertIsNotNone(post_step, "post step must still exist")
        post_block = "\n".join(self.source[post_step:post_step + 40])
        self.assertIn("check-runs", post_block,
                      "post step must still write to repos/.../check-runs")

    def test_self_test_trigger_reaches_this_contract(self) -> None:
        trigger = self_test_lines()
        on_block = block(trigger, "on", 0)
        pull_request = block(on_block, "pull_request", 2)
        paths = block(pull_request, "paths", 4)
        self.assertIn(
            "      - 'scripts/ci/test_ripr_liveness_pagination_16153.py'",
            paths,
            "self-test workflow must include the new contract test on "
            "pull_request paths so a regression on this file surfaces on the PR",
        )

    def test_a_one_page_snapshot_is_rejected(self) -> None:
        """Negative control: the defect class is a single-page snapshot."""
        # Strip the second-page call and the jq-merge. The remaining script
        # must no longer satisfy the contract, so a future regression that
        # drops pagination is caught.
        mutated = self.run_script
        mutated = re.sub(
            r'\s*gh api "repos/\$\{GITHUB_REPOSITORY\}/actions/workflows/'
            r'ripr\.yml/runs\?per_page=100&page=2" \\\n.*?\.\[0:\d+\]',
            "",
            mutated,
            flags=re.DOTALL,
        )
        mutated_pages = re.findall(
            r"gh api[^\n]*?workflows/ripr\.yml/runs\?[^\n]*",
            mutated,
        )
        self.assertLess(
            len(mutated_pages), 2,
            "removing the second-page call must drop the page count below 2 "
            "so the contract fires fail-closed; the regex above is the "
            "intended mutation surface",
        )


if __name__ == "__main__":
    unittest.main()
