#!/usr/bin/env python3
"""Focused tests for scripts/ci/ripr_liveness.py.

The classifier decides whether a ripr run that has scheduled no jobs is a
designed wait or an absence of proof, and that decision posts a real check run
on a real pull request. Two properties therefore matter more than coverage:

* a run queued behind its own predecessor must never be reported as a failure,
  because redding a pull request for waiting its turn is the defect class this
  exists to reduce;
* no input may produce a ``success`` conclusion, because this reporter knows
  only that a job was not scheduled and nothing about the candidate.

Both are pinned below against synthetic snapshots, plus the measured case that
motivated the script: run 35487554523 on PR #16083, pending with zero jobs for
45 minutes at the time of measurement.
"""

from __future__ import annotations

import contextlib
import datetime
import importlib.util
import io
import json
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

SCRIPT = Path(__file__).with_name("ripr_liveness.py")
SPEC = importlib.util.spec_from_file_location("ripr_liveness", SCRIPT)
assert SPEC and SPEC.loader
liveness = importlib.util.module_from_spec(SPEC)
sys.modules["ripr_liveness"] = liveness
SPEC.loader.exec_module(liveness)


def run(
    run_id: int,
    *,
    status: str = "queued",
    created_at: str = "2026-09-20T04:00:00Z",
    job_count: int | None = 0,
    pulls: list[int] | None = None,
    head_sha: str = "2698f3026226d52c7dcbc47d3dacf53e08465f75",
    head_branch: str = "claude/project-thread-1itw8h",
    event: str = "pull_request",
    head_repository: str = "EffortlessMetrics/perl-lsp-swarm",
    base_refs: list[str] | None = None,
) -> dict:
    return {
        "id": run_id,
        "status": status,
        "event": event,
        "created_at": created_at,
        "head_sha": head_sha,
        "head_branch": head_branch,
        "head_repository": head_repository,
        "pull_requests": [] if pulls is None else pulls,
        "base_refs": [] if base_refs is None else base_refs,
        "job_count": job_count,
    }


def snapshot(*runs: dict, as_of: str = "2026-09-20T04:35:00Z", floor: int = 10) -> dict:
    return {"as_of": as_of, "floor_minutes": floor, "runs": list(runs)}


class ClassificationTests(unittest.TestCase):
    def test_a_run_pending_with_no_jobs_and_no_predecessor_is_infra_no_proof(self) -> None:
        """The measured case: run 35487554523 on PR #16083, 45 min, zero jobs."""
        report = liveness.classify_snapshot(
            snapshot(
                run(
                    35487554523,
                    status="pending",
                    created_at="2026-09-20T03:49:05Z",
                    pulls=[16083],
                )
            )
        )
        self.assertEqual(len(report["findings"]), 1)
        finding = report["findings"][0]
        self.assertEqual(finding["classification"], liveness.INFRA_NO_PROOF)
        self.assertEqual(finding["conclusion"], "failure")
        self.assertEqual(finding["waited_minutes"], 45)
        self.assertIsNone(finding["predecessor_run_id"])

    def test_a_run_queued_behind_its_own_predecessor_is_not_a_failure(self) -> None:
        report = liveness.classify_snapshot(
            snapshot(
                run(200, status="queued", created_at="2026-09-20T04:00:00Z", pulls=[16083]),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    pulls=[16083],
                ),
            )
        )
        findings = report["findings"]
        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0]["run_id"], 200)
        self.assertEqual(findings[0]["classification"], liveness.SERIALISED)
        self.assertEqual(findings[0]["conclusion"], "neutral")
        self.assertEqual(findings[0]["predecessor_run_id"], 100)

    def test_a_queued_sibling_is_not_a_predecessor(self) -> None:
        """Only a started run can be holding the concurrency group."""
        report = liveness.classify_snapshot(
            snapshot(
                run(200, created_at="2026-09-20T04:00:00Z", pulls=[16083]),
                run(100, status="queued", created_at="2026-09-20T03:30:00Z", pulls=[16083]),
            )
        )
        classifications = {f["run_id"]: f["classification"] for f in report["findings"]}
        self.assertEqual(classifications[200], liveness.INFRA_NO_PROOF)
        self.assertEqual(classifications[100], liveness.INFRA_NO_PROOF)

    def test_a_predecessor_past_the_one_page_boundary_is_still_found(self) -> None:
        """Per #16153: a predecessor that fell off page one before #16153
        landed would be invisible to the classifier; with the multi-page
        snapshot, ``predecessor_for`` must still scan past the historical
        100-run boundary and report ``serialised_behind_predecessor``.

        The fixture places a same-group in-progress predecessor after 149
        unrelated completed runs on other pull requests. Without
        pagination the predecessor would not appear in this snapshot at
        all, so this test discriminates the fix from a no-op.
        """
        filler = [
            run(
                run_id=300 + idx,
                status="completed",
                created_at="2026-09-20T03:30:00Z",
                job_count=4,
                pulls=[16100 + (idx % 25)],
            )
            for idx in range(149)
        ]
        report = liveness.classify_snapshot(
            snapshot(
                run(200, created_at="2026-09-20T04:00:00Z", pulls=[16083]),
                *filler,
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    pulls=[16083],
                ),
            )
        )
        findings = report["findings"]
        self.assertEqual(len(findings), 1)
        finding = findings[0]
        self.assertEqual(finding["run_id"], 200)
        self.assertEqual(finding["classification"], liveness.SERIALISED,
                         "predecessor at position 150+ in the snapshot must "
                         "still serialise the queued candidate")
        self.assertEqual(finding["conclusion"], "neutral")
        self.assertEqual(finding["predecessor_run_id"], 100)

    def test_no_predecessor_anywhere_remains_infra_no_proof(self) -> None:
        """Control for #16153: a candidate with no predecessor in the
        multi-page snapshot must still classify ``infra-no-proof``. The fix
        cannot simply suppress the alarm.
        """
        filler = [
            run(
                run_id=400 + idx,
                status="completed",
                created_at="2026-09-20T03:30:00Z",
                job_count=4,
                pulls=[16100 + (idx % 25)],
            )
            for idx in range(149)
        ]
        report = liveness.classify_snapshot(
            snapshot(
                run(200, created_at="2026-09-20T04:00:00Z", pulls=[16083]),
                *filler,
            )
        )
        findings = report["findings"]
        self.assertEqual(len(findings), 1)
        finding = findings[0]
        self.assertEqual(finding["classification"], liveness.INFRA_NO_PROOF,
                         "without a same-group predecessor the candidate "
                         "must still alarm, otherwise the fix is just "
                         "suppressing the warning")
        self.assertEqual(finding["conclusion"], "failure")
        self.assertIsNone(finding["predecessor_run_id"])

    def test_an_in_progress_run_on_another_pull_request_explains_nothing(self) -> None:
        """`concurrency: ripr-<pr>` scopes the group; another PR does not contend."""
        report = liveness.classify_snapshot(
            snapshot(
                run(200, created_at="2026-09-20T04:00:00Z", pulls=[16083]),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    pulls=[16099],
                ),
            )
        )
        self.assertEqual(len(report["findings"]), 1)
        self.assertEqual(report["findings"][0]["classification"], liveness.INFRA_NO_PROOF)

    def test_push_runs_share_a_group_by_branch(self) -> None:
        """With no pull request the group falls back to the ref."""
        report = liveness.classify_snapshot(
            snapshot(
                run(200, created_at="2026-09-20T04:00:00Z", head_branch="main", event="push"),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    head_branch="main",
                    event="push",
                ),
            )
        )
        self.assertEqual(len(report["findings"]), 1)
        self.assertEqual(report["findings"][0]["classification"], liveness.SERIALISED)
        self.assertEqual(report["findings"][0]["predecessor_run_id"], 100)

    def test_a_run_with_scheduled_jobs_is_not_reported(self) -> None:
        report = liveness.classify_snapshot(
            snapshot(run(200, created_at="2026-09-20T03:00:00Z", job_count=3, pulls=[16083]))
        )
        self.assertEqual(report["findings"], [])

    def test_a_run_that_has_started_is_not_reported(self) -> None:
        """A job count of zero on a started run is a read race, not a fault."""
        report = liveness.classify_snapshot(
            snapshot(
                run(
                    200,
                    status="in_progress",
                    created_at="2026-09-20T03:00:00Z",
                    job_count=0,
                    pulls=[16083],
                )
            )
        )
        self.assertEqual(report["findings"], [])

    def test_the_floor_is_inclusive_at_its_boundary(self) -> None:
        """Exactly at the floor is past it; one minute short of it is not."""
        at_floor = liveness.classify_snapshot(
            snapshot(run(200, created_at="2026-09-20T04:25:00Z", pulls=[16083]))
        )
        self.assertEqual(at_floor["findings"][0]["waited_minutes"], 10)
        self.assertEqual(at_floor["findings"][0]["classification"], liveness.INFRA_NO_PROOF)

        inside = liveness.classify_snapshot(
            snapshot(run(200, created_at="2026-09-20T04:26:00Z", pulls=[16083]))
        )
        self.assertEqual(inside["findings"], [])

    def test_an_unreadable_job_count_is_treated_as_scheduled(self) -> None:
        """A failed jobs API read must not become evidence that nothing ran."""
        report = liveness.classify_snapshot(
            snapshot(run(200, created_at="2026-09-20T03:00:00Z", job_count=None, pulls=[16083]))
        )
        self.assertEqual(report["findings"], [])

    def test_a_run_with_no_readable_created_at_is_unclassifiable(self) -> None:
        report = liveness.classify_snapshot(
            snapshot(run(200, created_at="not a timestamp", pulls=[16083]))
        )
        self.assertEqual(report["findings"], [])
        self.assertEqual(len(report["unclassifiable"]), 1)
        self.assertEqual(report["unclassifiable"][0]["run_id"], 200)

    def test_a_snapshot_with_no_readable_as_of_classifies_nothing(self) -> None:
        report = liveness.classify_snapshot(
            snapshot(run(200, pulls=[16083]), as_of="whenever")
        )
        self.assertEqual(report["findings"], [])
        self.assertEqual(len(report["unclassifiable"]), 1)

    def test_a_missing_floor_falls_back_to_the_default(self) -> None:
        report = liveness.classify_snapshot(
            {"as_of": "2026-09-20T04:35:00Z", "runs": [run(200, pulls=[16083])]}
        )
        self.assertEqual(report["floor_minutes"], liveness.DEFAULT_FLOOR_MINUTES)


class ForkAndCoercionTests(unittest.TestCase):
    def test_two_fork_runs_sharing_a_branch_name_are_not_one_group(self) -> None:
        """The API withholds `pull_requests` for a fork PR; a branch name is
        not a substitute. `patch-1` is the commonest branch name there is, so
        the head repository has to carry the identity."""
        report = liveness.classify_snapshot(
            snapshot(
                run(
                    200,
                    created_at="2026-09-20T04:00:00Z",
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[],
                ),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    head_branch="patch-1",
                    head_repository="bob/perl-lsp-swarm",
                    pulls=[],
                ),
            )
        )
        self.assertEqual(len(report["findings"]), 1)
        finding = report["findings"][0]
        self.assertEqual(finding["run_id"], 200)
        self.assertEqual(finding["classification"], liveness.INFRA_NO_PROOF)
        self.assertIsNone(finding["predecessor_run_id"])

    def test_one_fork_pull_requests_own_predecessor_is_still_found(self) -> None:
        """The other direction, and the one that costs a false red.

        Both runs of a single fork pull request come back with no
        `pull_requests`, so comparing only the number rejects the real
        predecessor and reports the second push as `infra-no-proof` for
        queueing behind the first. Head repository plus branch identifies it.
        """
        report = liveness.classify_snapshot(
            snapshot(
                run(
                    200,
                    created_at="2026-09-20T04:00:00Z",
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[],
                    base_refs=["main"],
                ),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[],
                    base_refs=["main"],
                ),
            )
        )
        self.assertEqual(len(report["findings"]), 1)
        finding = report["findings"][0]
        self.assertEqual(finding["run_id"], 200)
        self.assertEqual(finding["classification"], liveness.SERIALISED)
        self.assertEqual(finding["predecessor_run_id"], 100)
        self.assertEqual(finding["conclusion"], "neutral")

    def test_one_fork_branch_with_two_bases_is_two_groups(self) -> None:
        """The correction #16109 review found.

        GitHub allows one open pull request per head **and base** pair, so a
        single fork branch can carry two at once -- one onto `main`, one onto
        `master`, both of which this repository's workflows accept. They have
        different numbers, so `ripr-<pr>` puts them in different concurrency
        groups and neither can be holding the other's slot.

        Matching on head repository plus branch alone explained the stalled
        run with the other pull request's run, which is the silent failure:
        a genuinely dead gate reported as an explained wait. With the base in
        the identity the stall is reported.
        """
        report = liveness.classify_snapshot(
            snapshot(
                run(
                    200,
                    created_at="2026-09-20T04:00:00Z",
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[],
                    base_refs=["main"],
                ),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[],
                    base_refs=["master"],
                ),
            )
        )
        self.assertEqual(len(report["findings"]), 1)
        finding = report["findings"][0]
        self.assertEqual(finding["run_id"], 200)
        self.assertEqual(finding["classification"], liveness.INFRA_NO_PROOF)
        self.assertIsNone(finding["predecessor_run_id"])

    def test_two_numbered_runs_on_one_branch_do_not_match_across_numbers(self) -> None:
        """Once the number is known it decides, and it decides against.

        The snapshot resolves the number for a run the API left bare, so the
        fallback is a last resort rather than the normal path. Two runs that
        agree on head repository and branch but carry different numbers are
        two pull requests, whatever else they share.
        """
        report = liveness.classify_snapshot(
            snapshot(
                run(
                    200,
                    created_at="2026-09-20T04:00:00Z",
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[4242],
                ),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[4243],
                ),
            )
        )
        self.assertEqual(len(report["findings"]), 1)
        finding = report["findings"][0]
        self.assertEqual(finding["run_id"], 200)
        self.assertEqual(finding["classification"], liveness.INFRA_NO_PROOF)

    def test_an_unresolvable_base_matches_nothing(self) -> None:
        """No number and no base is no identity, and no identity matches.

        The resolution call can fail. When it does the run is reported rather
        than silently grouped with whatever shares its branch name: a false
        `infra-no-proof` on an advisory check is visible and correctable, a
        suppressed one is neither.
        """
        report = liveness.classify_snapshot(
            snapshot(
                run(
                    200,
                    created_at="2026-09-20T04:00:00Z",
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[],
                ),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[],
                ),
            )
        )
        self.assertEqual(len(report["findings"]), 1)
        self.assertEqual(report["findings"][0]["classification"], liveness.INFRA_NO_PROOF)

    def test_an_ambiguous_base_set_is_not_an_identity(self) -> None:
        """Two bases for one head is the ambiguity, not a value to pick from."""
        self.assertIsNone(
            liveness.fork_identity(
                run(1, head_repository="alice/x", head_branch="patch-1",
                    base_refs=["main", "master"])
            )
        )
        self.assertEqual(
            liveness.fork_identity(
                run(1, head_repository="alice/x", head_branch="patch-1", base_refs=["main"])
            ),
            ("alice/x", "patch-1", "main"),
        )


class PredecessorTests(unittest.TestCase):
    def test_a_queued_sibling_that_has_jobs_is_a_predecessor(self) -> None:
        """A run claims its concurrency group on admission, not at first job
        start. A sibling whose jobs exist but are all waiting on a busy runner
        pool reads `queued` and is holding the group. Requiring `in_progress`
        reported the newest head as a failure during exactly the runner
        backlog that caused the wait.
        """
        report = liveness.classify_snapshot(
            snapshot(
                run(200, created_at="2026-09-20T04:00:00Z", pulls=[16099]),
                run(
                    100,
                    status="queued",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    pulls=[16099],
                ),
            )
        )
        self.assertEqual(len(report["findings"]), 1)
        finding = report["findings"][0]
        self.assertEqual(finding["run_id"], 200)
        self.assertEqual(finding["classification"], liveness.SERIALISED)
        self.assertEqual(finding["predecessor_run_id"], 100)

    def test_a_sibling_with_no_jobs_still_explains_nothing(self) -> None:
        """The other half of the same predicate: a sibling that has scheduled
        nothing either is not holding anything for anyone.
        """
        report = liveness.classify_snapshot(
            snapshot(
                run(200, created_at="2026-09-20T04:00:00Z", pulls=[16099]),
                run(
                    100,
                    status="queued",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=0,
                    pulls=[16099],
                ),
            )
        )
        findings = {finding["run_id"]: finding for finding in report["findings"]}
        self.assertEqual(findings[200]["classification"], liveness.INFRA_NO_PROOF)
        self.assertIsNone(findings[200]["predecessor_run_id"])

    def test_a_newer_run_is_not_a_predecessor(self) -> None:
        """`predecessor` is the word the posted title and summary use --
        "queued behind an earlier run". Without the ordering, a genuinely dead
        run reclassifies to an explained wait the moment its own replacement
        starts, naming a successor that reports on a different head and will
        never produce proof for this one. That is a real `infra-no-proof`
        suppressed by a false `serialised`.
        """
        report = liveness.classify_snapshot(
            snapshot(
                run(100, created_at="2026-09-20T04:00:00Z", pulls=[16099]),
                run(
                    200,
                    status="in_progress",
                    created_at="2026-09-20T04:10:00Z",
                    job_count=4,
                    pulls=[16099],
                ),
            )
        )
        self.assertEqual(len(report["findings"]), 1)
        finding = report["findings"][0]
        self.assertEqual(finding["run_id"], 100)
        self.assertEqual(finding["classification"], liveness.INFRA_NO_PROOF)
        self.assertIsNone(finding["predecessor_run_id"])

    def test_one_known_number_decides_against_a_bare_run(self) -> None:
        """The fallback is for when neither side has a number. One side
        knowing its own is enough to decide, and it decides against: a run
        carrying a number the other does not share is a different pull
        request, whatever branch they agree on.
        """
        report = liveness.classify_snapshot(
            snapshot(
                run(
                    200,
                    created_at="2026-09-20T04:00:00Z",
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[],
                    base_refs=["main"],
                ),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[16099],
                    base_refs=["main"],
                ),
            )
        )
        self.assertEqual(len(report["findings"]), 1)
        self.assertEqual(report["findings"][0]["classification"], liveness.INFRA_NO_PROOF)


class AwaitingApprovalTests(unittest.TestCase):
    """The fork-PR approval hold.

    ``waiting`` is the status GitHub gives a workflow run held for maintainer
    approval on a fork pull request. Reporting it as ``infra-no-proof``
    attributes a maintainer decision to the scheduler and degrades the signal
    this reporter exists to provide (#16151). These tests pin the new
    ``awaiting_approval`` classification against the cases that used to be
    misclassified.
    """

    def test_a_waiting_run_past_the_floor_is_awaiting_approval_not_failure(self) -> None:
        """The defect: this used to classify as ``infra-no-proof``/``failure``."""
        report = liveness.classify_snapshot(
            snapshot(
                run(
                    200,
                    status="waiting",
                    created_at="2026-09-20T04:00:00Z",
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[],
                    base_refs=["main"],
                )
            )
        )
        self.assertEqual(len(report["findings"]), 1)
        finding = report["findings"][0]
        self.assertEqual(finding["classification"], liveness.AWAITING_APPROVAL)
        self.assertEqual(finding["conclusion"], "neutral")
        self.assertIsNone(finding["predecessor_run_id"])
        self.assertIn("fork-PR approval", finding["check_title"])

    def test_a_waiting_run_inside_the_floor_is_silent(self) -> None:
        """The floor still gates the advisory report, regardless of cause."""
        report = liveness.classify_snapshot(
            snapshot(
                run(
                    200,
                    status="waiting",
                    created_at="2026-09-22T04:55:00Z",
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[],
                    base_refs=["main"],
                ),
                as_of="2026-09-22T05:00:00Z",
            )
        )
        self.assertEqual(report["findings"], [])

    def test_a_waiting_run_does_not_consult_predecessors(self) -> None:
        """The approval hold is a human gate, not concurrency contention.

        A ``waiting`` run cannot be holding a slot for any other run, and no
        run on the same pull request can be holding a slot for it. Looking up
        a predecessor for it would only churn through ``same_concurrency_group``
        on a classification that cannot use the answer, so the new branch
        short-circuits before ``predecessor_for`` (#16151).
        """
        # Even when a same-PR sibling has jobs, the waiting run is reported as
        # awaiting approval, not as serialised behind that sibling.
        report = liveness.classify_snapshot(
            snapshot(
                run(
                    200,
                    status="waiting",
                    created_at="2026-09-20T04:00:00Z",
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[],
                    base_refs=["main"],
                ),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[],
                    base_refs=["main"],
                ),
            )
        )
        findings = {f["run_id"]: f for f in report["findings"]}
        self.assertEqual(findings[200]["classification"], liveness.AWAITING_APPROVAL)
        self.assertIsNone(findings[200]["predecessor_run_id"])

    def test_pending_and_queued_are_unaffected_by_the_approval_branch(self) -> None:
        """The new branch is keyed on status, not on UNSTARTED_STATUSES membership.

        A ``pending`` run past the floor still classifies as ``infra-no-proof``
        (the measured case on PR #16083); a ``queued`` run past the floor
        still falls through to predecessor lookup.
        """
        pending = liveness.classify_snapshot(
            snapshot(run(200, status="pending", created_at="2026-09-20T04:00:00Z", pulls=[42]))
        )
        self.assertEqual(pending["findings"][0]["classification"], liveness.INFRA_NO_PROOF)
        self.assertEqual(pending["findings"][0]["conclusion"], "failure")

        queued_with_predecessor = liveness.classify_snapshot(
            snapshot(
                run(200, status="queued", created_at="2026-09-20T04:00:00Z", pulls=[42]),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    pulls=[42],
                ),
            )
        )
        self.assertEqual(
            queued_with_predecessor["findings"][0]["classification"],
            liveness.SERIALISED,
        )

    def test_the_awaiting_approval_summary_names_the_human_remedy(self) -> None:
        """The summary must name the remedy so the reader knows it's actionable."""
        report = liveness.classify_snapshot(
            snapshot(
                run(
                    200,
                    status="waiting",
                    created_at="2026-09-20T04:00:00Z",
                    head_branch="patch-1",
                    head_repository="alice/perl-lsp-swarm",
                    pulls=[],
                    base_refs=["main"],
                )
            )
        )
        finding = report["findings"][0]
        summary = finding["check_summary"]
        # The summary names the human remedy and tells the reader it is not
        # an infrastructure problem, instead of pinning the misclassification
        # we are replacing.
        self.assertIn("fork", summary.lower())
        self.assertIn("maintainer", summary.lower())
        self.assertNotEqual(finding["classification"], liveness.INFRA_NO_PROOF)
        self.assertEqual(finding["conclusion"], "neutral")

    def test_awaiting_approval_is_in_the_reportable_set_with_neutral_conclusion(self) -> None:
        """Sanity-check the constant surface, matching the SafetyTests contract."""
        self.assertIn(liveness.AWAITING_APPROVAL, liveness.REPORTABLE)
        self.assertEqual(liveness.CONCLUSIONS[liveness.AWAITING_APPROVAL], "neutral")
        # `waiting` must remain in UNSTARTED_STATUSES so the snapshot step
        # still reads job counts for approval-held runs.
        self.assertIn("waiting", liveness.UNSTARTED_STATUSES)


class ResolvedPullsTests(unittest.TestCase):
    def test_a_failed_read_is_unreadable_not_empty(self) -> None:
        self.assertIsNone(liveness.resolved_pulls_from_api(1, "[]"))
        self.assertIsNone(liveness.resolved_pulls_from_api(0, None))
        self.assertIsNone(liveness.resolved_pulls_from_api(0, "not json"))
        self.assertIsNone(liveness.resolved_pulls_from_api(0, '{"number": 1}'))

    def test_a_list_of_objects_is_an_answer(self) -> None:
        self.assertEqual(liveness.resolved_pulls_from_api(0, "[]"), [])
        self.assertEqual(
            liveness.resolved_pulls_from_api(0, '[{"number": 7, "base": "main"}, 3]'),
            [{"number": 7, "base": "main"}],
        )


class EndpointResolutionToGroupingTests(unittest.TestCase):
    """Fixtures spanning the whole seam: a `/commits/<sha>/pulls` payload as the
    endpoint would return it, through `apply_resolved_identity`, into
    `same_concurrency_group`.

    Hand-built singleton `pulls=[n]` lists cannot exercise this path -- they
    start on the far side of the decision under test, which is what let the
    collision survive the previous round (#16109 review).
    """

    SHARED_HEAD = "S" * 40
    # One fork commit, two open pull requests: PR4242 onto main, PR4243 onto
    # master. The endpoint returns both associations for the one head.
    ENDPOINT_BODY = '[{"number": 4242, "base": "main"}, {"number": 4243, "base": "master"}]'

    def _resolve(self, body: str, **kwargs) -> dict:
        """A run as the snapshot step would leave it after the resolution call."""
        candidate = run(kwargs.pop("run_id"), head_sha=self.SHARED_HEAD, **kwargs)
        candidate["pull_requests"] = []
        candidate["base_refs"] = []
        liveness.apply_resolved_identity(
            candidate, liveness.resolved_pulls_from_api(0, body)
        )
        return candidate

    def test_one_head_on_two_pull_requests_is_not_an_identity(self) -> None:
        resolved = self._resolve(self.ENDPOINT_BODY, run_id=1)
        self.assertTrue(liveness.has_ambiguous_identity(resolved))
        self.assertEqual(liveness.pull_numbers(resolved), [])
        self.assertIsNone(liveness.fork_identity(resolved))

    def test_two_runs_on_one_shared_head_do_not_share_a_group(self) -> None:
        """The finding itself. Both runs resolve the same head to the same two
        numbers, so a set intersection calls them a match -- and the base refs
        that would have separated them are never reached."""
        earlier = self._resolve(self.ENDPOINT_BODY, run_id=1, status="in_progress", job_count=3)
        later = self._resolve(self.ENDPOINT_BODY, run_id=2)
        self.assertFalse(liveness.same_concurrency_group(later, earlier))
        self.assertIsNone(liveness.predecessor_for(later, [earlier, later]))

    def test_an_unambiguous_resolution_still_groups(self) -> None:
        """The control: the fix must not make every resolved fork run
        unmatchable, which would pass the test above for the wrong reason."""
        body = '[{"number": 4242, "base": "main"}]'
        earlier = self._resolve(body, run_id=1, status="in_progress", job_count=3)
        later = self._resolve(body, run_id=2)
        self.assertEqual(liveness.pull_numbers(later), [4242])
        self.assertTrue(liveness.same_concurrency_group(later, earlier))
        self.assertEqual(liveness.predecessor_for(later, [earlier, later]), 1)

    def test_two_different_heads_resolving_to_one_number_each_still_separate(self) -> None:
        body_a = '[{"number": 4242, "base": "main"}]'
        body_b = '[{"number": 4243, "base": "master"}]'
        left = self._resolve(body_a, run_id=1, status="in_progress", job_count=3)
        right = self._resolve(body_b, run_id=2)
        self.assertFalse(liveness.same_concurrency_group(right, left))

    def test_an_unreadable_resolution_records_nothing(self) -> None:
        candidate = run(1, head_sha=self.SHARED_HEAD)
        candidate["pull_requests"] = []
        liveness.apply_resolved_identity(candidate, liveness.resolved_pulls_from_api(1, "[]"))
        self.assertEqual(liveness.pull_numbers(candidate), [])
        self.assertFalse(liveness.has_ambiguous_identity(candidate))

    def test_a_multi_number_api_array_is_ambiguous_too(self) -> None:
        """The other source of the same shape: the runs listing itself can
        carry more than one number, and that is no more an identity than a
        multi-result resolution is."""
        self.assertTrue(liveness.has_ambiguous_identity(run(1, pulls=[4242, 4243])))
        self.assertFalse(liveness.has_ambiguous_identity(run(1, pulls=[4242])))



    def test_a_fork_run_with_no_head_repository_matches_nothing(self) -> None:
        """An unidentifiable run must not match; an honest `infra-no-proof`
        naming no predecessor beats silently suppressing a real stall."""
        report = liveness.classify_snapshot(
            snapshot(
                run(
                    200,
                    created_at="2026-09-20T04:00:00Z",
                    head_branch="patch-1",
                    head_repository="",
                    pulls=[],
                ),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    head_branch="patch-1",
                    head_repository="",
                    pulls=[],
                ),
            )
        )
        self.assertEqual(report["findings"][0]["classification"], liveness.INFRA_NO_PROOF)
        self.assertIsNone(report["findings"][0]["predecessor_run_id"])

    def test_a_push_run_never_groups_with_a_pull_request_run(self) -> None:
        report = liveness.classify_snapshot(
            snapshot(
                run(200, created_at="2026-09-20T04:00:00Z", head_branch="main", event="push"),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    head_branch="main",
                    pulls=[16083],
                ),
            )
        )
        self.assertEqual(report["findings"][0]["classification"], liveness.INFRA_NO_PROOF)

    def test_a_blank_body_is_unreadable_rather_than_a_confirmed_zero(self) -> None:
        """A successful call returning nothing must not become a posted
        failure on a healthy run."""
        self.assertIsNone(liveness.job_count_from_api(0, ""))
        self.assertIsNone(liveness.job_count_from_api(0, "   \n"))
        self.assertIsNone(liveness.job_count_from_api(0, "null"))
        self.assertIsNone(liveness.job_count_from_api(0, "not a number"))
        self.assertIsNone(liveness.job_count_from_api(1, "4"))
        self.assertIsNone(liveness.job_count_from_api(0, None))

    def test_a_readable_count_is_taken_at_face_value(self) -> None:
        self.assertEqual(liveness.job_count_from_api(0, "0"), 0)
        self.assertEqual(liveness.job_count_from_api(0, " 7\n"), 7)


class SafetyTests(unittest.TestCase):
    def test_no_classification_can_produce_a_success_conclusion(self) -> None:
        self.assertNotIn("success", set(liveness.CONCLUSIONS.values()))
        self.assertEqual(set(liveness.CONCLUSIONS), set(liveness.REPORTABLE))

    def test_the_report_never_offers_the_required_context_as_a_check_name(self) -> None:
        report = liveness.classify_snapshot(
            snapshot(
                run(200, created_at="2026-09-20T04:00:00Z", pulls=[16083]),
                run(
                    300,
                    created_at="2026-09-20T04:00:00Z",
                    pulls=[16099],
                    head_sha="a" * 40,
                ),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    pulls=[16099],
                ),
            )
        )
        self.assertEqual(len(report["findings"]), 2)
        for finding in report["findings"]:
            self.assertEqual(finding["check_name"], liveness.ADVISORY_CHECK_NAME)
            self.assertNotEqual(finding["check_name"], liveness.REQUIRED_CONTEXT_NAME)
            self.assertNotEqual(finding["conclusion"], "success")

    def test_the_summary_names_the_absent_context_without_claiming_to_be_it(self) -> None:
        report = liveness.classify_snapshot(
            snapshot(run(200, created_at="2026-09-20T04:00:00Z", pulls=[16083]))
        )
        summary = report["findings"][0]["check_summary"]
        self.assertIn(liveness.REQUIRED_CONTEXT_NAME, summary)
        self.assertIn("is not the required context", summary)

    def test_a_serialised_wait_says_it_is_designed_and_names_the_predecessor(self) -> None:
        report = liveness.classify_snapshot(
            snapshot(
                run(200, created_at="2026-09-20T04:00:00Z", pulls=[16083]),
                run(
                    100,
                    status="in_progress",
                    created_at="2026-09-20T03:30:00Z",
                    job_count=4,
                    pulls=[16083],
                ),
            )
        )
        summary = report["findings"][0]["check_summary"]
        self.assertIn("designed behaviour, not a fault", summary)
        self.assertIn("100", summary)
        self.assertNotIn(liveness.INFRA_NO_PROOF, summary)


class RenderTests(unittest.TestCase):
    def test_a_quiet_report_says_so_rather_than_rendering_an_empty_table(self) -> None:
        markdown = liveness.render(liveness.classify_snapshot(snapshot()))
        self.assertIn("Every run has scheduled jobs", markdown)
        self.assertNotIn("| run |", markdown)

    def test_a_finding_renders_one_row_carrying_its_classification(self) -> None:
        markdown = liveness.render(
            liveness.classify_snapshot(
                snapshot(run(200, created_at="2026-09-20T04:00:00Z", pulls=[16083]))
            )
        )
        self.assertIn("| `200` |", markdown)
        self.assertIn("35 min", markdown)
        self.assertIn(liveness.INFRA_NO_PROOF, markdown)


class CliTests(unittest.TestCase):
    def test_main_writes_both_artifacts_and_returns_zero(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            snapshot_path = root / "snapshot.json"
            snapshot_path.write_text(
                json.dumps(
                    snapshot(run(200, created_at="2026-09-20T04:00:00Z", pulls=[16083]))
                ),
                encoding="utf-8",
            )
            out = root / "nested" / "report.md"
            json_out = root / "nested" / "report.json"

            # Exit zero on a finding, deliberately: the finding is delivered as
            # a check run on the affected head, and failing this job would red
            # the reporter rather than the run it is reporting on.
            self.assertEqual(
                liveness.main(
                    [
                        "--snapshot",
                        str(snapshot_path),
                        "--out",
                        str(out),
                        "--json",
                        str(json_out),
                    ]
                ),
                0,
            )
            written = json.loads(json_out.read_text(encoding="utf-8"))
            self.assertEqual(written["kind"], "ripr_liveness_report")
            self.assertEqual(len(written["findings"]), 1)
            self.assertIn("| `200` |", out.read_text(encoding="utf-8"))


HEAD = "2698f3026226d52c7dcbc47d3dacf53e08465f75"
OTHER_HEAD = "a" * 40
STALL_ID = f"ripr-liveness:{35487554523}:infra-no-proof"


def stalled(as_of: str = "2026-09-20T04:35:00Z") -> dict:
    """The measured case: run 35487554523 on PR #16083, waiting with no jobs."""
    return run(
        35487554523,
        status="pending",
        created_at="2026-09-20T03:49:05Z",
        pulls=[16083],
        head_sha=HEAD,
    )


def recovered(*, as_of: str = "2026-09-20T05:05:00Z", **overrides) -> dict:
    """The same run, now working: the API is the only record of the change."""
    candidate = run(
        35487554523,
        status="in_progress",
        created_at="2026-09-20T03:49:05Z",
        job_count=4,
        pulls=[16083],
        head_sha=HEAD,
    )
    candidate["progress"] = {
        "total_count": 4,
        "completed_count": 2,
        "in_progress_steps": ["Generate PR evidence"],
    }
    candidate.update(overrides)
    return candidate


def posted(ids: dict[str, int], head: str = HEAD) -> dict:
    records = {}
    for external_id, check_id in ids.items():
        identity = liveness.parse_external_id(external_id)
        run_id, classification = identity if identity is not None else (None, None)
        records[external_id] = [{
            "id": check_id,
            "conclusion": "failure" if classification == liveness.INFRA_NO_PROOF else "neutral",
            "title": f"ripr run {run_id} alive: fixture" if classification == liveness.ALIVE else None,
        }]
    return {head: records}


class RetractionTests(unittest.TestCase):
    """A stall that clears must stop being red on the head.

    The reporter was append-only: it could add a check run and never take one
    back, so a red it wrote outlived the condition that justified it. These
    tests are two-snapshot fixtures -- stall, then recovery -- over the same
    run, because the defect is a *transition* and neither snapshot alone can
    express one.
    """

    def test_a_stall_that_clears_is_withdrawn(self) -> None:
        """The defect. Snapshot A writes a failure; snapshot B withdraws it."""
        first = liveness.classify_snapshot(
            snapshot(stalled(), as_of="2026-09-20T04:35:00Z")
        )
        self.assertEqual(first["findings"][0]["conclusion"], "failure")
        external_id = first["findings"][0]["external_id"]
        self.assertEqual(external_id, STALL_ID)

        second = liveness.classify_snapshot(
            snapshot(recovered(), as_of="2026-09-20T05:05:00Z"),
            posted({STALL_ID: 55501}),
        )
        self.assertEqual(len(second["resolutions"]), 1)
        resolution = second["resolutions"][0]
        # The same external id, and the numeric id of the check run that
        # carries the red: a retraction has to *replace* that run, not sit
        # beside it as a second, contradicting statement.
        self.assertEqual(resolution["external_id"], STALL_ID)
        self.assertEqual(resolution["check_run_id"], 55501)
        self.assertEqual(resolution["conclusion"], "neutral")
        self.assertEqual(resolution["now_classification"], liveness.ALIVE)
        self.assertIn("no longer stalled", resolution["check_title"])

    def test_a_run_still_stalled_is_not_withdrawn(self) -> None:
        """The control, and the one that costs a false clear.

        Retracting on every pass would clear a live stall, which is strictly
        worse than the defect being fixed: the reporter would be telling a
        reader a dead run is fine.
        """
        report = liveness.classify_snapshot(
            snapshot(stalled(), as_of="2026-09-20T05:05:00Z"),
            posted({STALL_ID: 55501}),
        )
        self.assertEqual(report["resolutions"], [])
        self.assertEqual(len(report["findings"]), 1)
        self.assertEqual(report["findings"][0]["conclusion"], "failure")

    def test_an_unreadable_job_count_does_not_retract_a_live_stall(self) -> None:
        """The false retraction, and the reason `recovery_reason` exists.

        `classify_run` maps an unreadable count to `SCHEDULED`, which is right
        for reporting and exactly wrong for retraction. If "I could not read
        this" were treated as "the condition cleared", a still-dead run's red
        would be withdrawn, then re-reded on the next fire, flapping
        neutral/failure every 15 minutes while the gate is down. That reads as
        the fix working.
        """
        unreadable = run(
            35487554523,
            status="pending",
            created_at="2026-09-20T03:49:05Z",
            job_count=None,
            pulls=[16083],
            head_sha=HEAD,
        )
        report = liveness.classify_snapshot(
            snapshot(unreadable, as_of="2026-09-20T05:05:00Z"),
            posted({STALL_ID: 55501}),
        )
        self.assertEqual(report["resolutions"], [])
        # And the run is reported for neither, because nothing was established.
        self.assertEqual(report["findings"], [])
        self.assertIsNone(liveness.recovery_reason(unreadable, liveness.SCHEDULED))

    def test_a_run_back_inside_a_raised_floor_is_not_withdrawn(self) -> None:
        """The floor is a dispatch input, so it can move under a live stall.

        A `workflow_dispatch` with a larger `floor_minutes` reclassifies a still
        empty run to `within_floor`. That is a change in this reporter's own
        threshold, not a recovery of the run, and withdrawing the red on it
        would hide a stall that is still real.
        """
        still_empty = run(
            35487554523,
            status="pending",
            created_at="2026-09-20T03:49:05Z",
            pulls=[16083],
            head_sha=HEAD,
        )
        report = liveness.classify_snapshot(
            snapshot(still_empty, as_of="2026-09-20T03:55:00Z", floor=60),
            posted({STALL_ID: 55501}),
        )
        self.assertEqual(report["resolutions"], [])
        self.assertIsNone(
            liveness.recovery_reason(still_empty, liveness.WITHIN_FLOOR)
        )

    def test_a_run_that_gained_jobs_is_withdrawn(self) -> None:
        """`queued` with jobs is recovery, and is the runner-backlog shape.

        A run whose jobs exist but are all waiting on a busy pool reads
        `queued` and classifies `SCHEDULED`, so this path does not go through
        any of the three reportable classes -- and it is the empty-ness, not
        the class, that the original failure was about.
        """
        gained = run(
            35487554523,
            status="queued",
            created_at="2026-09-20T03:49:05Z",
            job_count=4,
            pulls=[16083],
            head_sha=HEAD,
        )
        report = liveness.classify_snapshot(
            snapshot(gained, as_of="2026-09-20T05:05:00Z"),
            posted({STALL_ID: 55501}),
        )
        self.assertEqual(len(report["resolutions"]), 1)
        self.assertEqual(report["resolutions"][0]["reason"], liveness.RESOLVED)

    def test_a_retraction_never_claims_a_run_finished_while_it_has_not(self) -> None:
        """Every reason's body must agree with its own title.

        The body used to fall through to "the run has finished" for any class
        it had no phrase for -- including a `queued` run with jobs, which is the
        single most reachable recovery and is emphatically not finished. The
        check run then contradicted itself on a live CI run.
        """
        cases = {
            liveness.RESOLVED: run(
                1, status="queued", job_count=4, pulls=[1], head_sha=HEAD
            ),
            liveness.RESOLVED_RUNNING: run(
                2, status="in_progress", job_count=4, pulls=[1], head_sha=HEAD
            ),
            liveness.RESOLVED_APPROVAL: run(
                3, status="waiting", pulls=[], head_branch="p",
                head_repository="a/b", base_refs=["main"], head_sha=HEAD
            ),
            liveness.RESOLVED_COMPLETED: run(
                4, status="completed", job_count=4, pulls=[1], head_sha=HEAD
            ),
        }
        for reason, candidate in cases.items():
            with self.subTest(reason=reason):
                summary = liveness.resolution_summary(candidate, reason)
                finished = "has finished" in summary
                self.assertEqual(
                    finished,
                    reason == liveness.RESOLVED_COMPLETED,
                    f"summary for {reason!r} asserts the wrong run state: {summary!r}",
                )

    def test_every_retraction_reason_has_a_body(self) -> None:
        """A reason with no body would raise inside the classifier.

        `RESOLUTION_BODIES[reason]` is a hard index, so a new reason added to
        `recovery_reason` without a body is a KeyError on a live fire rather
        than a failed assertion here.
        """
        reasons = {
            liveness.RESOLVED,
            liveness.RESOLVED_RUNNING,
            liveness.RESOLVED_QUEUED,
            liveness.RESOLVED_APPROVAL,
            liveness.RESOLVED_COMPLETED,
        }
        for reason in reasons:
            self.assertIn(reason, liveness.RESOLUTION_BODIES)
            self.assertTrue(liveness.RESOLUTION_BODIES[reason].strip())
        # And nothing that is not a reason may appear as a key.
        self.assertEqual(
            set(liveness.RESOLUTION_BODIES),
            reasons,
            "a body with no matching reason is dead text the classifier cannot emit",
        )

    def test_an_unknown_reason_raises_rather_than_defaulting(self) -> None:
        """The hard index is the safety property, and it needs its own test.

        Turning `RESOLUTION_BODIES[reason]` into a `.get(reason, <default>)`
        would look harmless -- and the default is "the run has finished", so a
        reason the table has lost would state, on a live CI run, that the run
        had completed. Nothing else in the suite would notice: every test that
        renders a body uses a reason the table still has.
        """
        candidate = run(1, status="queued", job_count=4, pulls=[1], head_sha=HEAD)
        with self.assertRaises(KeyError):
            liveness.resolution_summary(candidate, "a reason nobody defined")

    def test_an_unreadable_clock_does_not_raise_on_a_naive_datetime(self) -> None:
        """A public seam should not raise a TypeError into the snapshot step."""
        import datetime as _dt

        runs = [run(1, status="completed", created_at="2026-09-19T00:00:00Z", head_sha=HEAD)]
        self.assertEqual(
            liveness.posted_memory_heads(runs, _dt.datetime(2026, 9, 20, 12, 0)),
            {HEAD},
        )

    def test_the_report_states_how_many_heads_it_read(self) -> None:
        """The per-fire cost is reported, not assumed.

        A reader of the step summary should be able to see the read growing
        with throughput without going to the logs.
        """
        report = liveness.classify_snapshot(
            snapshot(recovered(), as_of="2026-09-20T05:05:00Z"),
            posted({STALL_ID: 55501}),
        )
        self.assertEqual(report["heads_read"], 1)
        self.assertIn("1 head(s) read", liveness.render(report))

    def test_a_recovered_run_is_not_withdrawn_without_a_record_of_the_failure(self) -> None:
        """No memory means no retraction.

        A recovery must not be reported as one on the strength of having
        happened at all: without the check run to withdraw, "resolved" is a
        claim about a write that was never made.
        """
        report = liveness.classify_snapshot(
            snapshot(recovered(), as_of="2026-09-20T05:05:00Z")
        )
        self.assertEqual(report["resolutions"], [])

    def test_a_stall_written_on_another_head_is_not_withdrawn(self) -> None:
        """A check run lives on the head it was posted against.

        A pushed head makes the old head stale, and the red on the stale head is
        a fact about that head. Retracting it from the new head's report would
        rewrite history rather than withdraw a claim.
        """
        report = liveness.classify_snapshot(
            snapshot(recovered(), as_of="2026-09-20T05:05:00Z"),
            posted({STALL_ID: 55501}, head=OTHER_HEAD),
        )
        self.assertEqual(report["resolutions"], [])

    def test_a_failure_for_a_different_classification_is_not_withdrawn(self) -> None:
        """Only `infra-no-proof` is red, so only it has a failure to take back.

        A posted `serialised` or `alive` run is neutral, and "withdrawing" it
        would be an update that changes nothing while implying a fault cleared.
        """
        report = liveness.classify_snapshot(
            snapshot(recovered(), as_of="2026-09-20T05:05:00Z"),
            posted({"ripr-liveness:35487554523:serialised_behind_predecessor": 55502}),
        )
        self.assertEqual(report["resolutions"], [])

    def test_a_resolution_names_what_the_run_reads_as_now(self) -> None:
        """The reader has to learn *why* it cleared, not just that it did."""
        report = liveness.classify_snapshot(
            snapshot(recovered(), as_of="2026-09-20T05:05:00Z"),
            posted({STALL_ID: 55501}),
        )
        summary = report["resolutions"][0]["check_summary"]
        self.assertIn("no longer holds", summary)
        self.assertIn("not a statement that the run produced proof", summary)
        # A retraction must not read as proof. If the word appears at all it
        # is to disclaim it, never to claim it.
        self.assertNotIn("succeeded", summary)

    def test_a_completed_run_is_withdrawn(self) -> None:
        """The commonest recovery: the run finished."""
        finished = run(
            35487554523,
            status="completed",
            created_at="2026-09-20T03:49:05Z",
            job_count=4,
            pulls=[16083],
            head_sha=HEAD,
        )
        report = liveness.classify_snapshot(
            snapshot(finished, as_of="2026-09-20T05:05:00Z"),
            posted({STALL_ID: 55501}),
        )
        self.assertEqual(len(report["resolutions"]), 1)
        self.assertIn("completed", report["resolutions"][0]["check_title"])

    def test_a_run_that_became_an_explained_wait_is_withdrawn(self) -> None:
        """A stall that turns out to be a serialised wait was never a fault.

        The reader is told it cleared and told why, so the earlier red reads as
        a superseded misreading rather than an unexplained change of mind.
        """
        behind = run(
            35487554523,
            status="queued",
            created_at="2026-09-20T03:49:05Z",
            pulls=[16083],
            head_sha=HEAD,
        )
        report = liveness.classify_snapshot(
            snapshot(
                behind,
                run(
                    35400000000,
                    status="in_progress",
                    created_at="2026-09-20T03:40:00Z",
                    job_count=4,
                    pulls=[16083],
                    head_sha=HEAD,
                ),
                as_of="2026-09-20T05:05:00Z",
            ),
            posted({STALL_ID: 55501}),
        )
        self.assertEqual(len(report["resolutions"]), 1)
        self.assertEqual(
            report["resolutions"][0]["now_classification"], liveness.SERIALISED
        )


class AliveTests(unittest.TestCase):
    """The positive case: a run that is working says so.

    Reading a run's health from ``updated_at`` does not work, because GitHub
    advances it only on run- and job-level transitions and not while a step
    executes -- a measured healthy run sat ~40 minutes inside one step, and a
    dead one is equally motionless.
    """

    def test_an_in_flight_run_reports_the_step_it_is_running(self) -> None:
        report = liveness.classify_snapshot(
            snapshot(recovered(), as_of="2026-09-20T05:05:00Z")
        )
        self.assertEqual(len(report["alive"]), 1)
        entry = report["alive"][0]
        self.assertEqual(entry["classification"], liveness.ALIVE)
        self.assertIn("Generate PR evidence", entry["check_title"])
        self.assertIn("2/4 jobs done", entry["check_title"])
        self.assertIn("75 min", entry["check_title"])

    def test_an_alive_run_is_not_a_finding(self) -> None:
        """`findings` means "a run reporting on absent proof".

        Folding a working run into it would hand every existing reader of that
        list -- the post loop, the markdown table, the early exit -- a fact it
        was never written for, and the early exit would stop firing on a
        genuinely quiet head.
        """
        report = liveness.classify_snapshot(
            snapshot(recovered(), as_of="2026-09-20T05:05:00Z")
        )
        self.assertEqual(report["findings"], [])
        self.assertEqual(len(report["alive"]), 1)

    def test_an_alive_run_never_carries_a_failure(self) -> None:
        """The safety property, stated as a fact about the data.

        `infra-no-proof` is the only `failure` in the table, and a run with
        jobs cannot reach it. This asserts the end of that chain rather than
        trusting the chain to hold.
        """
        report = liveness.classify_snapshot(
            snapshot(recovered(), as_of="2026-09-20T05:05:00Z")
        )
        for entry in report["alive"]:
            self.assertEqual(entry["conclusion"], "neutral")
            self.assertNotEqual(entry["conclusion"], "failure")
            self.assertNotEqual(entry["check_name"], liveness.REQUIRED_CONTEXT_NAME)

    def test_an_alive_report_refuses_to_read_as_proof(self) -> None:
        report = liveness.classify_snapshot(
            snapshot(recovered(), as_of="2026-09-20T05:05:00Z")
        )
        summary = report["alive"][0]["check_summary"]
        self.assertIn("not a verdict", summary)
        self.assertIn("must not be read as the run having produced proof", summary)

    def test_an_unreadable_progress_read_still_reports_alive(self) -> None:
        """"We could not read the step" is not "nothing is running".

        Dropping the run instead would make a transient read failure
        indistinguishable from a run making no progress, which is the exact
        confusion this reporter exists to remove.
        """
        candidate = run(
            35487554523,
            status="in_progress",
            created_at="2026-09-20T03:49:05Z",
            job_count=4,
            pulls=[16083],
            head_sha=HEAD,
        )
        report = liveness.classify_snapshot(
            snapshot(candidate, as_of="2026-09-20T05:05:00Z")
        )
        self.assertEqual(len(report["alive"]), 1)
        entry = report["alive"][0]
        self.assertIsNone(entry["progress"])
        self.assertIn("no step detail", entry["check_title"])
        self.assertIn("not evidence of a stall", entry["check_summary"])

    def test_a_queued_run_with_jobs_is_not_alive(self) -> None:
        """`queued` with jobs is waiting on a runner, not working.

        `serialised` already describes a designed wait, and calling that
        "alive" would report a run as making progress on the strength of jobs
        that have not started.
        """
        queued = run(
            35487554523,
            status="queued",
            created_at="2026-09-20T03:49:05Z",
            job_count=4,
            pulls=[16083],
            head_sha=HEAD,
        )
        report = liveness.classify_snapshot(
            snapshot(queued, as_of="2026-09-20T05:05:00Z")
        )
        self.assertEqual(report["alive"], [])

    def test_a_healthy_run_still_precedes_a_stalled_one(self) -> None:
        """The existing predecessor contract is unchanged by the alive channel.

        The healthy run carries a lower id because `predecessor` means *earlier*
        run; a later run is a successor and explains nothing, which is its own
        proven case.
        """
        healthy = run(
            100,
            status="in_progress",
            created_at="2026-09-20T03:30:00Z",
            job_count=4,
            pulls=[16099],
        )
        report = liveness.classify_snapshot(
            snapshot(
                run(200, created_at="2026-09-20T04:00:00Z", pulls=[16099]),
                healthy,
                as_of="2026-09-20T05:05:00Z",
            )
        )
        findings = {f["run_id"]: f for f in report["findings"]}
        self.assertEqual(findings[200]["classification"], liveness.SERIALISED)
        self.assertEqual(findings[200]["predecessor_run_id"], 100)
        self.assertEqual([e["run_id"] for e in report["alive"]], [100])


class ExternalIdTests(unittest.TestCase):
    """The id is the only thing that survives between cron fires."""

    def test_an_id_round_trips(self) -> None:
        built = liveness.external_id_for(42, liveness.INFRA_NO_PROOF)
        self.assertEqual(built, "ripr-liveness:42:infra-no-proof")
        self.assertEqual(
            liveness.parse_external_id(built), (42, liveness.INFRA_NO_PROOF)
        )

    def test_an_unreadable_run_id_has_no_external_id(self) -> None:
        """A stringified guess would post a check run nothing could withdraw."""
        self.assertIsNone(liveness.external_id_for(None, liveness.INFRA_NO_PROOF))
        self.assertIsNone(liveness.external_id_for("42", liveness.INFRA_NO_PROOF))
        self.assertIsNone(liveness.external_id_for(True, liveness.INFRA_NO_PROOF))

    def test_only_this_reporters_own_namespace_parses(self) -> None:
        """A check run written by any other job is not this reporter's to touch.

        The prefix is the whole guard. Without it, a run id scraped out of an
        unrelated check run would let this reporter address a check run it does
        not own.
        """
        self.assertIsNone(liveness.parse_external_id("42:infra-no-proof"))
        self.assertIsNone(liveness.parse_external_id("other:42:infra-no-proof"))
        self.assertIsNone(liveness.parse_external_id("ripr-liveness:42"))
        self.assertIsNone(liveness.parse_external_id("ripr-liveness:x:y"))
        self.assertIsNone(liveness.parse_external_id("ripr-liveness:42:a:b"))
        self.assertIsNone(liveness.parse_external_id(None))

    def test_an_unreadable_head_reports_nothing_posted(self) -> None:
        """Failing toward "not posted" re-posts; the other way loses a signal."""
        self.assertIsNone(
            liveness.posted_id_for(posted({STALL_ID: 1}), None, 35487554523, liveness.INFRA_NO_PROOF)
        )
        self.assertIsNone(
            liveness.posted_id_for({}, HEAD, 35487554523, liveness.INFRA_NO_PROOF)
        )
        self.assertIsNone(
            liveness.posted_id_for(posted({STALL_ID: 0}), HEAD, 35487554523, liveness.INFRA_NO_PROOF)
        )


class PostedChecksReadTests(unittest.TestCase):
    ROW = {"external_id": "ripr-liveness:1:infra-no-proof", "id": 9001,
           "conclusion": "failure", "title": None}

    def test_a_line_per_check_run_parses_to_an_addressable_state(self) -> None:
        self.assertEqual(
            liveness.posted_checks_from_api(0, json.dumps(self.ROW)),
            {self.ROW["external_id"]: [{"id": 9001, "conclusion": "failure", "title": None}]},
        )

    def test_paginated_duplicate_external_ids_retain_every_check(self) -> None:
        second = {**self.ROW, "id": 9002, "conclusion": "neutral"}
        body = "\n".join(json.dumps(row) for row in (self.ROW, second, self.ROW))
        self.assertEqual(
            liveness.posted_checks_from_api(0, body),
            {self.ROW["external_id"]: [
                {"id": 9001, "conclusion": "failure", "title": None},
                {"id": 9002, "conclusion": "neutral", "title": None},
            ]},
        )

    def test_a_failed_read_is_unreadable_not_empty(self) -> None:
        for code, body in ((1, json.dumps(self.ROW)), (0, None), (0, "\n\n")):
            with self.subTest(code=code, body=body):
                self.assertIsNone(liveness.posted_checks_from_api(code, body))

    def test_partial_or_malformed_rows_are_unreadable(self) -> None:
        for row in (
            {**self.ROW, "id": 0}, {**self.ROW, "id": -1}, {**self.ROW, "id": True},
            {**self.ROW, "external_id": None}, {**self.ROW, "external_id": ""},
            {**self.ROW, "conclusion": None}, {"external_id": "x"}, [1, 2, 3],
        ):
            with self.subTest(row=row):
                self.assertIsNone(liveness.posted_checks_from_api(0, json.dumps(row)))

    def test_an_unreadable_line_does_not_discard_the_usable_ones(self) -> None:
        body = "not json\n" + json.dumps(self.ROW) + "\n[1, 2, 3]\n"
        self.assertEqual(
            liveness.posted_checks_from_api(0, body),
            liveness.posted_checks_from_api(0, json.dumps(self.ROW)),
        )

    def test_open_progress_title_is_retained_for_later_retirement(self) -> None:
        row = {**self.ROW, "external_id": "ripr-liveness:1:alive",
               "conclusion": "neutral", "title": "ripr run 1 alive: running tests"}
        self.assertEqual(
            liveness.posted_checks_from_api(0, json.dumps(row)),
            {row["external_id"]: [{"id": 9001, "conclusion": "neutral", "title": row["title"]}]},
        )


class ProgressReadTests(unittest.TestCase):
    BODY = json.dumps(
        {
            "total_count": 4,
            "completed_count": 2,
            "in_progress_steps": ["Generate PR evidence"],
        }
    )

    def test_a_readable_detail_becomes_progress(self) -> None:
        self.assertEqual(
            liveness.progress_from_api(0, self.BODY),
            {
                "total_count": 4,
                "completed_count": 2,
                "in_progress_steps": ["Generate PR evidence"],
            },
        )

    def test_a_failed_read_is_unreadable(self) -> None:
        self.assertIsNone(liveness.progress_from_api(1, self.BODY))
        self.assertIsNone(liveness.progress_from_api(0, None))
        self.assertIsNone(liveness.progress_from_api(0, "not json"))
        self.assertIsNone(liveness.progress_from_api(0, "[]"))
        self.assertIsNone(liveness.progress_from_api(0, '{"total_count": -1}'))

    def test_a_completed_count_beyond_the_total_is_clamped(self) -> None:
        """The API races a job from running to complete between two counts."""
        self.assertEqual(
            liveness.progress_from_api(
                0, json.dumps({"total_count": 2, "completed_count": 3})
            )["completed_count"],
            2,
        )

    def test_a_run_with_no_step_running_still_reads_as_progress(self) -> None:
        """A read that succeeded and found nothing running is not a failed read.

        The two are different facts and collapse into the same "alive" class,
        so the title has to carry which one it was. Reporting "no step detail"
        here would be a lie about a read that answered.
        """
        between_steps = run(
            35487554523,
            status="in_progress",
            created_at="2026-09-20T03:49:05Z",
            job_count=4,
            pulls=[16083],
            head_sha=HEAD,
        )
        between_steps["progress"] = {
            "total_count": 4,
            "completed_count": 4,
            "in_progress_steps": [],
        }
        report = liveness.classify_snapshot(
            snapshot(between_steps, as_of="2026-09-20T05:05:00Z")
        )
        self.assertIn("no step running", report["alive"][0]["check_title"])
        self.assertIn("4/4 jobs done", report["alive"][0]["check_title"])


class TransitionSafetyTests(unittest.TestCase):
    """The refusals the issue requires a control for."""

    def test_no_path_in_the_report_can_carry_a_success_conclusion(self) -> None:
        """Not "the table has no success" -- the produced data has none either.

        A table is an intention; the rendered report is what gets written to a
        head. The retraction path is the one worth proving, because it is the
        only write that replaces a `failure` with something else.
        """
        cases = [
            (stalled(), {"as_of": "2026-09-20T05:05:00Z"}, None),
            (recovered(), {"as_of": "2026-09-20T05:05:00Z"}, posted({STALL_ID: 55501})),
            (
                run(7, status="waiting", created_at="2026-09-20T03:00:00Z",
                    head_branch="patch-1", head_repository="a/b", base_refs=["main"]),
                {"as_of": "2026-09-20T05:05:00Z"},
                None,
            ),
        ]
        for run_obj, kwargs, memory in cases:
            report = liveness.classify_snapshot(snapshot(run_obj, **kwargs), memory)
            for channel in ("findings", "alive", "resolutions"):
                for entry in report[channel]:
                    self.assertNotEqual(
                        entry["conclusion"],
                        "success",
                        msg=f"{channel} produced a success conclusion",
                    )

    def test_every_channel_offers_only_the_advisory_check_name(self) -> None:
        report = liveness.classify_snapshot(
            snapshot(recovered(), as_of="2026-09-20T05:05:00Z"),
            posted({STALL_ID: 55501}),
        )
        for channel in ("findings", "alive", "resolutions"):
            for entry in report[channel]:
                self.assertEqual(entry["check_name"], liveness.ADVISORY_CHECK_NAME)
                self.assertNotEqual(entry["check_name"], liveness.REQUIRED_CONTEXT_NAME)

    def test_the_only_failure_still_comes_only_from_an_empty_run(self) -> None:
        report = liveness.classify_snapshot(
            snapshot(
                recovered(),
                run(200, created_at="2026-09-20T04:00:00Z", pulls=[16099]),
                as_of="2026-09-20T05:05:00Z",
            ),
            posted({STALL_ID: 55501}),
        )
        failures = [
            entry
            for channel in ("findings", "alive", "resolutions")
            for entry in report[channel]
            if entry["conclusion"] == "failure"
        ]
        self.assertEqual([f["run_id"] for f in failures], [200])
        self.assertEqual(failures[0]["classification"], liveness.INFRA_NO_PROOF)

    def test_the_report_states_what_it_will_not_claim(self) -> None:
        report = liveness.classify_snapshot(snapshot(recovered()))
        boundary = " ".join(report["claim_boundary"])
        self.assertIn("Never emits a success conclusion", boundary)
        self.assertIn("never posts under the required context", boundary)
        self.assertIn("not a verdict", boundary)
        self.assertIn("never retracted", boundary)


class WorkflowTransitionTests(unittest.TestCase):
    """The workflow is transport. These assert it stays transport.

    The module makes the decisions; this file must not make a second, untested
    copy of any of them. The shipped defect this guards against is precisely a
    decision that lived in the YAML, where nothing could reach it (#16152).
    """

    WORKFLOW = Path(__file__).resolve().parents[2] / ".github" / "workflows" / "ripr-liveness.yml"

    def setUp(self) -> None:
        self.text = self.WORKFLOW.read_text(encoding="utf-8")

    def test_the_workflow_still_refuses_a_success_conclusion(self) -> None:
        """The refusal the issue requires a control for, kept verbatim in role."""
        self.assertIn('if conclusion == "success":', self.text)
        self.assertIn(
            "refusing to post a success conclusion from the liveness reporter", self.text
        )

    def test_the_workflow_still_refuses_the_required_context(self) -> None:
        self.assertIn("if name == REQUIRED_CONTEXT:", self.text)
        self.assertIn("refusing to post under the required context", self.text)

    def test_the_workflow_can_update_a_check_run_it_already_wrote(self) -> None:
        """The capability the defect was the absence of."""
        self.assertIn('"PATCH"', self.text)
        self.assertIn("/check-runs/{check_run_id}", self.text)

    def test_an_update_preserves_identity_and_sends_only_state(self) -> None:
        """The reporter updates an observed check without changing its identity."""
        patch_block = self.text.split("if verb == \"PATCH\" and isinstance(check_run_id, int):", 1)[1]
        patch_payload = patch_block.split("payload = {", 1)[1].split("}", 1)[0]
        self.assertNotIn("head_sha", patch_payload)
        self.assertNotIn("external_id", patch_payload)
        self.assertIn("conclusion", patch_payload)

    def test_the_post_loop_delegates_rather_than_deciding(self) -> None:
        """No `already_posted` in the YAML: that decision is now the module's."""
        self.assertNotIn("def already_posted", self.text)
        self.assertIn("posted_checks_from_api", self.text)

    def test_the_dedup_read_keeps_its_pinned_flags(self) -> None:
        """`filter=all` and `--paginate` are the fix for a shipped defect (#16344)."""
        self.assertIn("check-runs?filter=all&per_page=100", self.text)
        self.assertIn('"--paginate"', self.text)

    def test_the_snapshot_reads_step_detail_for_in_flight_runs(self) -> None:
        self.assertIn("IN_FLIGHT_STATUSES", self.text)
        self.assertIn("in_progress_steps", self.text)

    def test_the_classify_step_passes_the_posted_memory(self) -> None:
        self.assertIn("--posted target/ripr/liveness/posted.json", self.text)


class PostedMemoryBoundTests(unittest.TestCase):
    """Which heads the reporter bothers to read its own history for.

    The runs page carries 92 distinct heads over its last 100 entries, and the
    check-runs read is paginated. One call per head, every 15 minutes, is how an
    advisory reporter becomes the expensive thing it exists to make cheap --
    so the population is bounded deliberately, and these pin both halves of the
    bound against the mistake that would undo it.
    """

    AS_OF = "2026-09-20T12:00:00Z"

    def _head(self, run_id: int, head: str, **kwargs) -> dict:
        return run(run_id, head_sha=head, **kwargs)

    def _heads(self, runs: list[dict], as_of: str | None = None) -> set[str]:
        moment = liveness.parse_time(as_of or self.AS_OF)
        return liveness.posted_memory_heads(runs, moment)

    def test_a_run_still_going_is_worth_reading(self) -> None:
        self.assertIn(HEAD, self._heads([self._head(1, HEAD, status="in_progress")]))

    def test_a_run_that_just_finished_is_worth_reading(self) -> None:
        """The population that is easy to forget, and the one that matters.

        A run that stalls and then completes never becomes non-completed again,
        so bounding on "runs that have not finished" would silently skip the
        retraction for the commonest recovery of all.
        """
        finished = self._head(
            1, HEAD, status="completed", created_at="2026-09-20T11:00:00Z"
        )
        finished["updated_at"] = "2026-09-20T11:30:00Z"
        self.assertIn(HEAD, self._heads([finished]))

    def test_a_run_that_finished_long_ago_is_not_worth_reading(self) -> None:
        old = self._head(
            1, HEAD, status="completed", created_at="2026-09-19T00:00:00Z"
        )
        old["updated_at"] = "2026-09-19T01:00:00Z"
        self.assertEqual(self._heads([old]), set())

    def test_a_long_running_head_is_kept_independently_of_retention_age(self) -> None:
        active = self._head(1, HEAD, status="in_progress", created_at="2026-09-18T00:00:00Z")
        self.assertIn(HEAD, self._heads([active]))

    def test_the_post_completion_window_is_generous(self) -> None:
        """A long run retains the full window after its terminal update."""
        long_run = self._head(1, HEAD, status="completed", created_at="2026-09-19T00:00:00Z")
        long_run["updated_at"] = "2026-09-20T11:55:00Z"
        self.assertIn(HEAD, self._heads([long_run]))

    def test_only_relevant_heads_are_selected_from_a_full_page(self) -> None:
        """The shape that actually happens: 100 runs, 92 heads, a few live."""
        runs: list[dict] = []
        for index in range(77):
            old = self._head(
                    1000 + index,
                    f"{index:040x}",
                    status="completed",
                    created_at="2026-09-18T00:00:00Z",
                )
            old["updated_at"] = "2026-09-18T01:00:00Z"
            runs.append(old)
        live = self._head(9001, HEAD, status="in_progress")
        stalled = self._head(9002, OTHER_HEAD, status="pending")
        runs += [live, stalled]
        self.assertEqual(self._heads(runs), {HEAD, OTHER_HEAD})

    def test_an_unreadable_clock_reads_every_head(self) -> None:
        """Over-reading costs calls; under-reading skips a retraction.

        The two failure modes are not equally bad, so an unusable clock takes
        the expensive one.
        """
        runs = [
            self._head(1, HEAD, status="completed", created_at="2026-09-19T00:00:00Z")
        ]
        self.assertEqual(liveness.posted_memory_heads(runs, None), {HEAD})

    def test_a_headless_run_contributes_nothing(self) -> None:
        self.assertEqual(self._heads([{"id": 1, "status": "in_progress"}]), set())


class AliveFloorTests(unittest.TestCase):
    """`alive` is gated by the floor, like every other class here.

    18 ripr runs were in flight at once when this was written. A heartbeat on
    every one of them every 15 minutes is ~1,700 advisory writes a day, most of
    them announcing a run that had just started. The reader who needs this is
    the one asking whether a long run is dead, and the floor is that threshold.
    """

    def test_a_working_run_inside_the_floor_says_nothing(self) -> None:
        fresh = run(
            35487554523,
            status="in_progress",
            created_at="2026-09-20T04:30:00Z",
            job_count=4,
            pulls=[16083],
            head_sha=HEAD,
        )
        report = liveness.classify_snapshot(
            snapshot(fresh, as_of="2026-09-20T04:35:00Z")
        )
        self.assertEqual(report["alive"], [])
        self.assertEqual(report["findings"], [])

    def test_the_same_run_past_the_floor_is_alive(self) -> None:
        """The control, so the test above is not passing for the wrong reason."""
        older = run(
            35487554523,
            status="in_progress",
            created_at="2026-09-20T04:24:00Z",
            job_count=4,
            pulls=[16083],
            head_sha=HEAD,
        )
        report = liveness.classify_snapshot(
            snapshot(older, as_of="2026-09-20T04:35:00Z")
        )
        self.assertEqual(len(report["alive"]), 1)

    def test_the_floor_applies_to_alive_and_to_a_stall_alike(self) -> None:
        """One floor, one meaning: do not comment on a run that just started."""
        for status, job_count in (("in_progress", 4), ("pending", 0)):
            inside = run(
                7,
                status=status,
                created_at="2026-09-20T04:30:00Z",
                job_count=job_count,
                pulls=[16083],
            )
            report = liveness.classify_snapshot(
                snapshot(inside, as_of="2026-09-20T04:35:00Z")
            )
            self.assertEqual(report["alive"], [], status)
            self.assertEqual(report["findings"], [], status)


class ReconciliationStateTests(unittest.TestCase):
    def _row(self, check_id, *, external_id=STALL_ID, conclusion="failure", title=None):
        return {"external_id": external_id, "id": check_id,
                "conclusion": conclusion, "title": title}

    def _history(self, *rows):
        body = "\n".join(json.dumps(row) for row in rows)
        return {HEAD: liveness.posted_checks_from_api(0, body)}

    def _report(self, history, *runs):
        return liveness.classify_snapshot(snapshot(*runs, as_of="2026-09-20T05:05:00Z"), history)

    def test_recovery_retracts_only_failures_and_only_once(self) -> None:
        row = self._row(11)
        first = self._report(self._history(row), recovered())
        self.assertEqual([entry["check_run_id"] for entry in first["resolutions"]], [11])
        row["conclusion"] = first["resolutions"][0]["conclusion"]
        self.assertEqual(self._report(self._history(row), recovered())["resolutions"], [])

    def test_every_duplicate_failure_is_retracted_independently_of_read_order(self) -> None:
        rows = [self._row(13, conclusion="neutral"), self._row(12), self._row(11)]
        for ordered in (rows, list(reversed(rows))):
            report = self._report(self._history(*ordered), recovered())
            self.assertEqual([entry["check_run_id"] for entry in report["resolutions"]], [11, 12])

    def test_missing_conclusion_cannot_authorize_a_retraction(self) -> None:
        row = self._row(11)
        del row["conclusion"]
        self.assertEqual(self._report(self._history(row), recovered())["resolutions"], [])

    def test_legacy_id_only_memory_does_not_authorize_a_retraction(self) -> None:
        report = self._report({HEAD: {STALL_ID: 11}}, recovered())
        self.assertEqual(report["resolutions"], [])
        self.assertIsNone(report["alive"][0]["check_run_id"])

    def _open_alive(self, check_id=21):
        return self._row(
            check_id, external_id="ripr-liveness:35487554523:alive", conclusion="neutral",
            title="ripr run 35487554523 alive: running Generate PR evidence",
        )

    def test_completed_progress_observation_is_closed_once(self) -> None:
        row = self._open_alive()
        finished = recovered(status="completed")
        first = self._report(self._history(row), finished)
        self.assertEqual(len(first["resolutions"]), 1)
        close = first["resolutions"][0]
        self.assertEqual(close["check_run_id"], 21)
        self.assertEqual(close["classification"], liveness.ALIVE)
        self.assertEqual(close["conclusion"], "neutral")
        self.assertIn("progress observation ended (completed)", close["check_title"])
        self.assertNotIn("running Generate PR evidence", close["check_title"])
        self.assertIn("`alive`", liveness.render(first))
        row.update(conclusion=close["conclusion"], title=close["check_title"])
        self.assertEqual(self._report(self._history(row), finished)["resolutions"], [])

    def test_still_running_or_absent_progress_observation_is_not_closed(self) -> None:
        history = self._history(self._open_alive())
        self.assertEqual(self._report(history, recovered())["resolutions"], [])
        self.assertEqual(self._report(history)["resolutions"], [])

    def test_only_a_known_open_progress_title_is_closed(self) -> None:
        row = self._open_alive()
        for title in (None, "unrecognized title", "ripr run 35487554523: progress observation ended (completed)"):
            row["title"] = title
            self.assertEqual(self._report(self._history(row), recovered(status="completed"))["resolutions"], [])

    def test_every_open_progress_copy_is_closed_but_a_closed_copy_is_not(self) -> None:
        rows = [self._open_alive(21), self._open_alive(22), self._open_alive(23)]
        rows[2]["title"] = "ripr run 35487554523: progress observation ended (completed)"
        report = self._report(self._history(*rows), recovered(status="completed"))
        self.assertEqual([entry["check_run_id"] for entry in report["resolutions"]], [21, 22])

    def test_an_unknown_run_status_cannot_close_progress(self) -> None:
        report = self._report(self._history(self._open_alive()), recovered(status="unknown"))
        self.assertEqual(report["resolutions"], [])

    def test_queued_again_progress_is_closed_without_claiming_success(self) -> None:
        report = self._report(self._history(self._open_alive()), recovered(status="queued", job_count=0))
        self.assertEqual(len(report["resolutions"]), 1)
        self.assertEqual(report["resolutions"][0]["conclusion"], "neutral")

    def test_completed_retention_never_guesses_from_an_old_creation_time(self) -> None:
        finished = recovered(status="completed", created_at="2026-09-18T00:00:00Z")
        for updated_at in (None, "unreadable", "2026-09-20T05:00:00Z"):
            finished["updated_at"] = updated_at
            self.assertEqual(
                liveness.posted_memory_heads([finished], liveness.parse_time("2026-09-20T05:05:00Z")),
                {HEAD},
            )

    def _execute_post_step(self, report, *, forbid_network=False):
        text = WorkflowTransitionTests.WORKFLOW.read_text(encoding="utf-8")
        programs = [textwrap.dedent(part.split("\n          PY", 1)[0])
                    for part in text.split("python3 - <<'PY'\n")[1:]
                    if "def write(entry, verb):" in part]
        self.assertEqual(len(programs), 1)
        calls = []

        def request(args, **kwargs):
            self.assertFalse(forbid_network, "forbidden entry reached network")
            calls.append((args, json.loads(kwargs["input"])))
            return SimpleNamespace(returncode=0, stdout="", stderr="")

        with (mock.patch.dict("os.environ", {"GITHUB_REPOSITORY": "fixture/repository"}),
              mock.patch("builtins.open", mock.mock_open(read_data=json.dumps(report))),
              mock.patch("subprocess.run", side_effect=request),
              contextlib.redirect_stdout(io.StringIO())):
            try:
                exec(compile(programs[0], str(WorkflowTransitionTests.WORKFLOW), "exec"), {})
            except SystemExit as stopped:
                if stopped.code != 0:
                    raise
        return calls

    def test_duplicate_recovery_writes_each_exact_id_then_stops(self) -> None:
        rows = [self._row(11), self._row(12)]
        report = self._report(self._history(*rows), recovered(status="completed"))
        calls = self._execute_post_step(report)
        self.assertEqual([args[4] for args, _ in calls],
                         ["repos/fixture/repository/check-runs/11", "repos/fixture/repository/check-runs/12"])
        for row, (args, payload) in zip(rows, calls):
            self.assertEqual(args[3], "PATCH")
            self.assertEqual(payload["conclusion"], "neutral")
            self.assertNotIn("head_sha", payload)
            self.assertNotIn("external_id", payload)
            row["conclusion"] = payload["conclusion"]
        self.assertEqual(self._execute_post_step(self._report(self._history(*rows), recovered(status="completed"))), [])

    def test_post_step_preserves_both_forbidden_write_guards(self) -> None:
        report = self._report(self._history(self._row(11)), recovered(status="completed"))
        entry = report["resolutions"][0]
        for conclusion, name in (("success", liveness.ADVISORY_CHECK_NAME),
                                 ("neutral", liveness.REQUIRED_CONTEXT_NAME)):
            entry.update(conclusion=conclusion, check_name=name)
            with self.subTest(conclusion=conclusion, name=name), self.assertRaises(SystemExit):
                self._execute_post_step(report, forbid_network=True)


class CancelledBeforeSchedulingTests(unittest.TestCase):
    """A terminal run must replace the reporter's earlier promise (#19129)."""

    RUN_ID = 42106122702
    AS_OF = "2026-09-20T05:05:00Z"
    TERMINAL = "cancelled_before_scheduling"

    def candidate(self, **overrides):
        candidate = run(self.RUN_ID, status="completed", event="push",
                        head_branch="main", head_sha=HEAD)
        candidate.update(conclusion="cancelled", run_attempt=1,
                         updated_at="2026-09-20T05:00:00Z")
        candidate.update(overrides)
        return candidate

    def report(self, *runs, history=None):
        return liveness.classify_snapshot(snapshot(*runs, as_of=self.AS_OF), history)

    def history(self, classification=liveness.SERIALISED, *, check_id=71, head=HEAD,
                run_id=None, title="ripr queued behind an earlier run for 60 min",
                conclusion="neutral"):
        external_id = liveness.external_id_for(run_id or self.RUN_ID, classification)
        return {head: {external_id: [{"id": check_id, "conclusion": conclusion,
                                     "title": title}]}}

    def test_terminal_attempt_is_reported_even_before_the_wait_floor(self):
        candidate = self.candidate(created_at="2026-09-20T05:04:00Z",
                                   updated_at="2026-09-20T05:04:30Z")
        findings = self.report(candidate)["findings"]
        self.assertEqual(len(findings), 1)
        finding = findings[0]
        self.assertEqual(finding["classification"], self.TERMINAL)
        self.assertEqual(finding["conclusion"], "neutral")
        self.assertEqual(finding["check_name"], liveness.ADVISORY_CHECK_NAME)
        self.assertIn("cancelled before scheduling", finding["check_title"])
        summary = finding["check_summary"]
        self.assertIn("This attempt will not produce", summary)
        self.assertIn(liveness.REQUIRED_CONTEXT_NAME, summary)
        self.assertIn("#19128", summary)
        self.assertNotIn("the gate will report once", summary)

    def test_cancellation_is_not_a_permanent_claim_about_the_whole_sha(self):
        other = self.candidate(id=self.RUN_ID + 1, conclusion="success", job_count=4)
        findings = self.report(self.candidate(), other)["findings"]
        self.assertEqual([entry["run_id"] for entry in findings], [self.RUN_ID])
        summary = findings[0]["check_summary"]
        self.assertIn("another run", summary)
        self.assertNotIn("permanently absent", summary)

    def test_existing_wait_is_patched_at_its_exact_id_then_stops(self):
        history = self.history()
        first = self.report(self.candidate(), history=history)
        self.assertEqual([entry["check_run_id"] for entry in first["resolutions"]], [71])
        correction = first["resolutions"][0]
        self.assertEqual(correction["external_id"],
                         liveness.external_id_for(self.RUN_ID, liveness.SERIALISED))
        self.assertIn("cancelled", correction["check_summary"])
        self.assertIn("This attempt will not produce", correction["check_summary"])
        calls = ReconciliationStateTests._execute_post_step(self, first)
        patches = [(args, body) for args, body in calls if args[3] == "PATCH"]
        self.assertEqual(len(patches), 1)
        self.assertEqual(patches[0][0][4], "repos/fixture/repository/check-runs/71")
        self.assertNotIn("external_id", patches[0][1])
        self.assertNotIn("head_sha", patches[0][1])
        for _, body in calls:
            self.assertNotEqual(body["conclusion"], "success")
            self.assertNotEqual(body.get("name"), liveness.REQUIRED_CONTEXT_NAME)

        prior = next(iter(history[HEAD].values()))[0]
        prior.update(title=correction["check_title"], conclusion=correction["conclusion"])
        terminal = first["findings"][0]
        history[HEAD][terminal["external_id"]] = [
            {"id": 72, "conclusion": terminal["conclusion"], "title": terminal["check_title"]}
        ]
        second = self.report(self.candidate(), history=history)
        self.assertEqual(ReconciliationStateTests._execute_post_step(self, second), [])

    def test_approval_and_stall_records_are_corrected_without_guessing_zero_jobs(self):
        for prior_class, prior_conclusion in ((liveness.SERIALISED, "neutral"),
                                             (liveness.AWAITING_APPROVAL, "neutral"),
                                             (liveness.INFRA_NO_PROOF, "failure")):
            for count in (None, "0", False, -1, 3):
                with self.subTest(prior_class=prior_class, count=count):
                    report = self.report(self.candidate(job_count=count), history=self.history(
                        prior_class, conclusion=prior_conclusion))
                    self.assertEqual(report["findings"], [])
                    self.assertEqual(len(report["resolutions"]), 1)
                    summary = report["resolutions"][0]["check_summary"]
                    self.assertIn("cancelled", summary)
                    self.assertNotIn("before any job was scheduled", summary)
                    self.assertNotIn("the gate will report once", summary)

    def test_missing_or_invalid_attempt_identity_does_not_claim_zero_job_cancellation(self):
        for attempt in (None, 0, True, "1"):
            with self.subTest(attempt=attempt):
                self.assertEqual(self.report(self.candidate(run_attempt=attempt))["findings"], [])

    def test_other_terminal_or_scheduled_states_do_not_become_cancellation_findings(self):
        for overrides in ({"conclusion": "success"}, {"conclusion": "failure"},
                          {"conclusion": None}, {"job_count": 1}):
            with self.subTest(overrides=overrides):
                self.assertEqual(self.report(self.candidate(**overrides))["findings"], [])
        running = self.candidate(status="in_progress", conclusion=None, job_count=3)
        report = self.report(running)
        self.assertEqual(report["findings"], [])
        self.assertEqual([entry["classification"] for entry in report["alive"]], [liveness.ALIVE])

    def test_rerun_retires_terminal_record_but_other_run_or_head_cannot(self):
        terminal_title = self.report(self.candidate())["findings"][0]["check_title"]
        for status, conclusion, count in (("queued", None, 0),
                                           ("in_progress", None, 3),
                                           ("completed", "success", 3)):
            with self.subTest(status=status):
                history = self.history(self.TERMINAL, title=terminal_title)
                rerun = self.candidate(status=status, conclusion=conclusion,
                                       job_count=count, run_attempt=2)
                report = self.report(rerun, history=history)
                self.assertEqual([entry["check_run_id"] for entry in report["resolutions"]], [71])
                self.assertIn("no longer current", report["resolutions"][0]["check_summary"])
                self.assertEqual(report["resolutions"][0]["conclusion"], "neutral")
        history = self.history(self.TERMINAL, title=terminal_title)
        for changed in ({"id": self.RUN_ID + 1}, {"head_sha": "b" * 40}):
            candidate = self.candidate(status="completed", conclusion="success", job_count=3,
                                       run_attempt=2, **changed)
            self.assertEqual(self.report(candidate, history=history)["resolutions"], [])
        self.assertEqual(self.report(history=history)["resolutions"], [])

    def test_old_cancelled_attempt_stays_outside_the_existing_retention_window(self):
        old = self.candidate(created_at="2026-09-19T00:00:00Z",
                             updated_at="2026-09-19T00:05:00Z")
        report = self.report(old, history=self.history())
        self.assertEqual(report["findings"], [])
        self.assertEqual(report["resolutions"], [])

    def test_rerun_retires_every_cancellation_copy_including_corrected_waits(self):
        history = self.history()
        first = self.report(self.candidate(), history=history)
        closed_wait = first["resolutions"][0]
        terminal = first["findings"][0]
        for entry, check_id in ((closed_wait, 71), (terminal, 72), (terminal, 73)):
            history[HEAD].setdefault(entry["external_id"], [])
            history[HEAD][entry["external_id"]] = [
                row for row in history[HEAD][entry["external_id"]] if row["id"] != check_id
            ] + [{"id": check_id, "conclusion": entry["conclusion"], "title": entry["check_title"]}]
        rerun = self.candidate(status="completed", conclusion="success", job_count=4, run_attempt=2)
        report = self.report(rerun, history=history)
        self.assertEqual(sorted(entry["check_run_id"] for entry in report["resolutions"]), [71, 72, 73])
        for entry in report["resolutions"]:
            for row in history[HEAD][entry["external_id"]]:
                if row["id"] == entry["check_run_id"]:
                    row["title"] = entry["check_title"]
        self.assertEqual(self.report(rerun, history=history)["resolutions"], [])

    def test_another_cancelled_attempt_refreshes_all_duplicates_once(self):
        terminal = self.report(self.candidate())["findings"][0]
        history = self.history(self.TERMINAL, title=terminal["check_title"])
        history[HEAD][terminal["external_id"]].append(
            {"id": 72, "conclusion": "neutral", "title": terminal["check_title"]})
        again = self.candidate(run_attempt=2)
        findings = self.report(again, history=history)["findings"]
        self.assertEqual([entry["check_run_id"] for entry in findings], [71, 72])
        for entry, row in zip(findings, history[HEAD][terminal["external_id"]]):
            self.assertIn("attempt 2", entry["check_title"])
            row["title"] = entry["check_title"]
        self.assertEqual(self.report(again, history=history)["findings"], [])

    def cancelled_history(self):
        first = self.report(self.candidate(), history=self.history())
        history = {HEAD: {}}
        for entry, check_id in ((first["resolutions"][0], 71), (first["findings"][0], 72)):
            history[HEAD][entry["external_id"]] = [
                {"id": check_id, "conclusion": entry["conclusion"], "title": entry["check_title"]}
            ]
        return history

    def test_positive_count_or_a_new_cancelled_attempt_corrects_all_previous_zero_job_claims(self):
        for attempt, count in ((1, 3), (2, 3), (2, None)):
            with self.subTest(attempt=attempt, count=count):
                history = self.cancelled_history()
                candidate = self.candidate(run_attempt=attempt, job_count=count)
                report = self.report(candidate, history=history)
                self.assertEqual(report["findings"], [])
                self.assertEqual(sorted(entry["check_run_id"] for entry in report["resolutions"]), [71, 72])
                calls = ReconciliationStateTests._execute_post_step(self, report)
                self.assertEqual(len(calls), 2)
                for entry in report["resolutions"]:
                    self.assertNotIn("before scheduling", entry["check_title"])
                    self.assertNotIn("This attempt will not produce", entry["check_summary"])
                    self.assertIn(f"attempt {attempt}", entry["check_title"])
                    history[HEAD][entry["external_id"]][0]["title"] = entry["check_title"]
                self.assertEqual(self.report(candidate, history=history)["resolutions"], [])

    def test_unreadable_count_does_not_erase_same_attempt_terminal_evidence(self):
        for count in (None, "0", False, -1):
            with self.subTest(count=count):
                report = self.report(self.candidate(job_count=count), history=self.cancelled_history())
                self.assertEqual(report["findings"], [])
                self.assertEqual(report["resolutions"], [])

    def test_rerun_live_class_refresh_is_not_overwritten_by_retirement(self):
        for prior_class, conclusion in ((liveness.INFRA_NO_PROOF, "failure"),
                                         (liveness.SERIALISED, "neutral")):
            with self.subTest(prior_class=prior_class):
                history = self.history(prior_class, conclusion=conclusion)
                correction = self.report(self.candidate(), history=history)["resolutions"][0]
                history[HEAD][correction["external_id"]][0].update(
                    title=correction["check_title"], conclusion="neutral")
                rerun = self.candidate(status="queued", conclusion=None, run_attempt=2)
                runs = [rerun]
                if prior_class == liveness.SERIALISED:
                    runs.append(self.candidate(id=self.RUN_ID - 1, status="in_progress",
                                               conclusion=None, job_count=3))
                report = self.report(*runs, history=history)
                calls = ReconciliationStateTests._execute_post_step(self, report)
                writes = [body for args, body in calls
                          if args[4] == "repos/fixture/repository/check-runs/71"]
                self.assertEqual(len(writes), 1)
                self.assertEqual(writes[0]["conclusion"], conclusion)
                self.assertNotIn("cancellation observation ended", writes[0]["output"]["title"])

    def test_serialised_summary_reports_a_snapshot_without_promising_a_future_gate(self):
        candidate = self.candidate(status="pending", conclusion=None)
        predecessor = self.candidate(id=self.RUN_ID - 1, status="in_progress",
                                     conclusion=None, job_count=3)
        finding = self.report(candidate, predecessor)["findings"][0]
        self.assertEqual(finding["classification"], liveness.SERIALISED)
        self.assertNotIn("the gate will report once", finding["check_summary"])
        predecessor["status"] = "completed"
        self.assertEqual(self.report(candidate, predecessor)["findings"][0]["classification"],
                         liveness.INFRA_NO_PROOF)

    def test_a_wait_does_not_erase_another_run_result_on_the_same_sha(self):
        for predecessor_status, expected in (("in_progress", liveness.SERIALISED),
                                              ("completed", liveness.INFRA_NO_PROOF)):
            with self.subTest(predecessor_status=predecessor_status):
                candidate = self.candidate(status="pending", conclusion=None)
                predecessor = self.candidate(id=self.RUN_ID - 1, status=predecessor_status,
                                             conclusion=None, job_count=3, head_sha="b" * 40)
                finished = self.candidate(id=self.RUN_ID - 2, conclusion="success", job_count=4)
                finding = self.report(candidate, predecessor, finished)["findings"][0]
                self.assertEqual(finding["classification"], expected)
                summary = finding["check_summary"]
                self.assertIn("another run", summary)
                self.assertNotIn("absent** from this head", summary)
                self.assertNotIn("proof for this head", summary)

    def test_all_workflow_page_projections_keep_conclusion_and_attempt(self):
        if not shutil.which("jq"):
            self.skipTest("jq is required to execute the workflow's jq projections")
        text = WorkflowTransitionTests.WORKFLOW.read_text(encoding="utf-8")
        projections = re.findall(r"--jq '(\[\.workflow_runs\[\].*?)'", text, re.DOTALL)
        self.assertEqual(len(projections), 3)
        candidate = self.candidate()
        candidate["head_repository"] = {"full_name": "fixture/repository"}
        for index, projection in enumerate(projections):
            with self.subTest(page=index + 1):
                result = subprocess.run(["jq", projection], input=json.dumps({"workflow_runs": [candidate]}),
                                        capture_output=True, text=True, check=True)
                projected = json.loads(result.stdout)
                self.assertEqual(len(projected), 1)
                self.assertEqual(projected[0].get("conclusion"), "cancelled")
                self.assertEqual(projected[0].get("run_attempt"), 1)

    def execute_snapshot(self, candidates, *, count_returncode=0, count_stdout="0"):
        text = WorkflowTransitionTests.WORKFLOW.read_text(encoding="utf-8")
        programs = [textwrap.dedent(part.split("\n          PY", 1)[0])
                    for part in text.split("python3 - <<'PY'\n")[1:]
                    if "for run in runs:" in part]
        self.assertEqual(len(programs), 1)
        written, requests = {}, []

        def open_snapshot(name, mode="r", **kwargs):
            if "w" in mode:
                stream = io.StringIO()
                written[str(name)] = stream
                return stream
            self.assertEqual(str(name), "target/ripr/liveness/runs.json")
            return io.StringIO(json.dumps(candidates))

        def request(args, **kwargs):
            requests.append(args)
            if args[-2:] == ["--jq", ".total_count"]:
                return SimpleNamespace(returncode=count_returncode, stdout=count_stdout, stderr="")
            self.assertIn("/check-runs?", args[3])
            return SimpleNamespace(returncode=0, stdout="", stderr="")

        class SnapshotClock(datetime.datetime):
            @classmethod
            def now(cls, tz=None):
                return cls(2026, 9, 20, 5, 5, tzinfo=datetime.timezone.utc)

        with (mock.patch.dict("os.environ", {"GITHUB_REPOSITORY": "fixture/repository",
                                            "FLOOR_MINUTES": "10"}),
              mock.patch("builtins.open", side_effect=open_snapshot),
              mock.patch("importlib.util.spec_from_file_location", return_value=SPEC),
              mock.patch("subprocess.run", side_effect=request),
              mock.patch("datetime.datetime", SnapshotClock)):
            exec(compile(programs[0], str(WorkflowTransitionTests.WORKFLOW), "exec"), {})
        return json.loads(written["target/ripr/liveness/snapshot.json"].getvalue()), requests

    def test_snapshot_reads_exact_cancelled_attempt_without_spending_on_old_or_successful_runs(self):
        candidates = [self.candidate(), self.candidate(id=self.RUN_ID + 1, conclusion="success"),
                      self.candidate(id=self.RUN_ID + 2, updated_at="2026-09-19T00:00:00Z"),
                      self.candidate(id=self.RUN_ID + 3, run_attempt=None)]
        observed, requests = self.execute_snapshot(candidates)
        job_reads = [args[2] for args in requests if args[-2:] == ["--jq", ".total_count"]]
        self.assertEqual(job_reads, [f"repos/fixture/repository/actions/runs/{self.RUN_ID}/attempts/1/jobs"])
        self.assertEqual(observed["runs"][0]["job_count"], 0)
        report = liveness.classify_snapshot(observed)
        self.assertEqual([entry["classification"] for entry in report["findings"]], [self.TERMINAL])

    def test_snapshot_failed_job_read_does_not_invent_an_empty_attempt(self):
        observed, requests = self.execute_snapshot([self.candidate()],
                                                  count_returncode=1, count_stdout="")
        self.assertTrue(any("/attempts/1/jobs" in " ".join(args) for args in requests))
        self.assertIsNone(observed["runs"][0]["job_count"])
        self.assertEqual(liveness.classify_snapshot(observed)["findings"], [])


if __name__ == "__main__":
    unittest.main()
