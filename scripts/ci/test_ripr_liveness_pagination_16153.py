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
from typing import Any

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


def page_calls(script: str) -> list[str]:
    return re.findall(
        r"gh api[^\n]*?workflows/ripr\.yml/runs\?[^\n]*",
        script,
    )


# `per_page=100` contains the substring "page=", so the identity check looks
# for a real page parameter, not one embedded in the page-size parameter.
PAGE_PARAM = re.compile(r"(?<![A-Za-z_])page=")


def merge_workflow_pages(pages: list[list[dict[str, Any]]], *, bound: int = 300) -> list[dict[str, Any]]:
    """Mirror the Snapshot step's ``jq -s`` merge used in ripr-liveness.yml."""

    merged: list[dict[str, Any]] = []
    for page in pages:
        merged.extend(page)
    seen: dict[Any, dict[str, Any]] = {}
    for run in merged:
        seen.setdefault(run["id"], run)
    unique = list(seen.values())
    unique.sort(key=lambda run: run["created_at"])
    unique.reverse()
    return unique[:bound]


def offset_shift_fixture() -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]], int]:
    """Simulate a new run starting between the page-one and page-two fetches."""

    runs = [
        {"id": run_id, "created_at": f"2026-01-01T{run_id:02d}:00:00Z"}
        for run_id in range(1, 201)
    ]
    page_one = list(runs[:100])
    inserted = {"id": 0, "created_at": "2026-01-01T99:00:00Z"}
    shifted = [inserted, *runs]
    page_two = list(shifted[100:200])
    page_three = list(shifted[200:300])
    predecessor_id = 200
    return page_one, page_two, page_three, predecessor_id


class RiprLivenessPaginationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.source = lines()
        cls.run_script = "\n".join(find_run_blocks(cls.source))

    def assert_page_identity(self, script: str) -> None:
        """Exactly one unpaged call plus explicit `page=2` and `page=3`.

        Counting calls alone is not enough: retargeting the first call to
        `page=2` keeps three calls and a `per_page=100` each while silently
        dropping the newest 100 runs -- the exact omission this contract
        exists to catch.
        """
        calls = page_calls(script)
        self.assertEqual(
            len(calls), 3,
            "the snapshot step must make exactly three page calls so a "
            "worst-case predecessor (queue + ~31 min analysis) survives an "
            "offset shift at the two-hundred-run tail (#16386)",
        )
        self.assertEqual(
            sum(1 for call in calls if PAGE_PARAM.search(call)), 2,
            "exactly two calls must carry an explicit page parameter",
        )
        self.assertEqual(
            sum(1 for call in calls if not PAGE_PARAM.search(call)), 1,
            "exactly one call must be page one (no page parameter), so the "
            "newest runs cannot be silently dropped",
        )
        paged = [call for call in calls if PAGE_PARAM.search(call)]
        self.assertIn(
            "page=2",
            paged[0],
            "the first paged call must fetch page two explicitly; any other "
            "value reintroduces a wrong-window snapshot",
        )
        self.assertIn(
            "page=3",
            paged[1],
            "the trailing paged call must fetch page three so a run shifted "
            "off the page-two tail is still merged (#16386)",
        )
        for call in calls:
            self.assertIn(
                "per_page=100", call,
                "each call must request a full 100-run page; a smaller "
                "page size silently shrinks the scanned window",
            )

    def test_the_snapshots_step_fetches_more_than_one_page(self) -> None:
        """A single page caps at 100 runs; the predecessor can fall off it."""
        self.assert_page_identity(self.run_script)

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
            int(bound.group(1)), 200,
            "the bound must retain the fetched pages (at least 2 x 100 runs), "
            "otherwise a predecessor past the bound is truncated even "
            "though it was fetched: the one-page defect in merge form",
        )
        self.assertLessEqual(
            int(bound.group(1)), 1000,
            "the bound must stay modest; huge snapshots defeat the queueing "
            "argument that motivates this workflow",
        )

    def test_offset_shift_keeps_the_two_hundredth_predecessor(self) -> None:
        """A run inserted between page fetches must not drop the old tail (#16386)."""

        page_one, page_two, page_three, predecessor_id = offset_shift_fixture()
        two_page = merge_workflow_pages([page_one, page_two])
        three_page = merge_workflow_pages([page_one, page_two, page_three])
        merged_ids = {run["id"] for run in three_page}
        self.assertIn(
            predecessor_id,
            merged_ids,
            "the overlapping page-three fetch must retain the predecessor that "
            "offset pagination shifted off page two",
        )
        self.assertNotIn(
            predecessor_id,
            {run["id"] for run in two_page},
            "fixture must reproduce the two-page tail drop the workflow fix addresses",
        )

    def test_the_workflow_publishes_via_an_existing_check_run(self) -> None:
        """Posting lives in a later step, so the snapshot change cannot regress it."""
        post_step = next(
            (i for i, line in enumerate(self.source)
             if line.strip() == "- name: Post advisory check runs"),
            None,
        )
        self.assertIsNotNone(post_step, "post step must still exist")
        # Scan the whole step, not a fixed window: the step's inline script
        # grows with the posting logic, and a hard-coded line count goes stale
        # on unrelated main-side drift.
        post_end = next(
            (i for i in range(post_step + 1, len(self.source))
             if self.source[i].strip().startswith("- name:")),
            len(self.source),
        )
        post_block = "\n".join(self.source[post_step:post_end])
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

    def test_a_page_retarget_is_rejected(self) -> None:
        """Negative control: retargeting page one to `page=2` fails the contract.

        Two `page=2` calls satisfy every call-count assertion and still drop
        the newest 100 runs, so the identity check must be what fires.
        """
        mutated = self.run_script.replace(
            'runs?per_page=100" \\',
            'runs?per_page=100&page=2" \\',
            1,
        )
        self.assertNotEqual(
            mutated, self.run_script,
            "the retarget mutation must actually change the script",
        )
        with self.assertRaises(AssertionError):
            self.assert_page_identity(mutated)

    def test_a_wrong_page_value_is_rejected(self) -> None:
        """Negative control: paged calls must name pages two and three."""

        for wrong in ("page=1", "page=4"):
            mutated = self.run_script.replace("page=2", wrong, 1)
            self.assertNotEqual(
                mutated, self.run_script,
                "the page-two mutation must actually change the script",
            )
            with self.assertRaises(AssertionError, msg=f"{wrong} must fail"):
                self.assert_page_identity(mutated)

        for wrong in ("page=2", "page=4"):
            mutated = self.run_script.replace("page=3", wrong, 1)
            self.assertNotEqual(
                mutated, self.run_script,
                "the page-three mutation must actually change the script",
            )
            with self.assertRaises(AssertionError, msg=f"{wrong} must fail"):
                self.assert_page_identity(mutated)

    def test_a_smaller_page_size_is_rejected(self) -> None:
        """Negative control: per_page is part of the window arithmetic."""
        mutated = self.run_script.replace("per_page=100&page=2", "per_page=1&page=2", 1)
        self.assertNotEqual(
            mutated, self.run_script,
            "the page-size mutation must actually change the script",
        )
        with self.assertRaises(AssertionError):
            self.assert_page_identity(mutated)

    def test_a_one_page_snapshot_is_rejected(self) -> None:
        """Negative control: the defect class is a single-page snapshot."""
        # Strip the paged calls and the jq-merge. The remaining script
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
            len(mutated_pages), 3,
            "removing the paged calls must drop the page count below 3 "
            "so the contract fires fail-closed; the regex above is the "
            "intended mutation surface",
        )
        with self.assertRaises(AssertionError):
            self.assert_page_identity(mutated)


if __name__ == "__main__":
    unittest.main()
