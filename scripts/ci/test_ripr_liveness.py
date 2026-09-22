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

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

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


if __name__ == "__main__":
    unittest.main()
