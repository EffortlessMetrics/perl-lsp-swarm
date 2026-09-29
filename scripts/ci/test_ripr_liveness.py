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


def posted(ids: dict[str, int], head: str = HEAD) -> dict[str, dict[str, int]]:
    return {head: ids}


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
    """The read that supplies the memory between fires.

    ``filter=all`` and ``--paginate`` were the subject of the one defect this
    workflow shipped: the dedup read used the endpoint's default filter, which
    answers "is this the newest id under this name" when the question is "has
    this id ever been posted". Those two flags are wired in the workflow; what
    the body *means* is pinned here, because a parser that quietly accepted the
    wrong shape would look identical to a working one.
    """

    ROW = '{"external_id": "ripr-liveness:1:infra-no-proof", "id": 9001}'

    def test_a_line_per_check_run_parses_to_an_addressable_id(self) -> None:
        self.assertEqual(
            liveness.posted_checks_from_api(0, self.ROW + "\n"),
            {"ripr-liveness:1:infra-no-proof": 9001},
        )

    def test_paginated_output_is_one_row_per_line(self) -> None:
        body = "\n".join(
            [
                '{"external_id": "ripr-liveness:1:infra-no-proof", "id": 9001}',
                '{"external_id": "ripr-liveness:2:alive", "id": 9002}',
                "",
            ]
        )
        self.assertEqual(
            liveness.posted_checks_from_api(0, body),
            {
                "ripr-liveness:1:infra-no-proof": 9001,
                "ripr-liveness:2:alive": 9002,
            },
        )

    def test_a_failed_read_is_unreadable_not_empty(self) -> None:
        """`None` and `{}` are different answers and must not collapse.

        `None` lets the caller re-post, which is visible. An empty map would
        read as "nothing has ever been posted here" and silently stop updating
        a live signal.
        """
        self.assertIsNone(liveness.posted_checks_from_api(1, ""))
        self.assertIsNone(liveness.posted_checks_from_api(0, None))
        self.assertIsNone(liveness.posted_checks_from_api(0, "\n\n"))

    def test_a_row_without_an_addressable_id_is_dropped(self) -> None:
        """A partial row would aim an update at check run 0.

        The API's answer to that is a 404, indistinguishable from a row that
        was never this reporter's.
        """
        self.assertIsNone(
            liveness.posted_checks_from_api(0, '{"external_id": "ripr-liveness:1:x"}\n')
        )
        self.assertIsNone(
            liveness.posted_checks_from_api(0, '{"external_id": "ripr-liveness:1:x", "id": 0}\n')
        )
        self.assertIsNone(
            liveness.posted_checks_from_api(0, '{"external_id": null, "id": 9001}\n')
        )
        self.assertIsNone(
            liveness.posted_checks_from_api(0, '{"id": 9001}\n')
        )

    def test_an_unreadable_line_does_not_discard_the_usable_ones(self) -> None:
        body = "\n".join(
            [
                "not json at all",
                '{"external_id": "ripr-liveness:1:infra-no-proof", "id": 9001}',
                "[1, 2, 3]",
            ]
        )
        self.assertEqual(
            liveness.posted_checks_from_api(0, body),
            {"ripr-liveness:1:infra-no-proof": 9001},
        )

    def test_the_query_selects_only_rows_carrying_an_external_id(self) -> None:
        """A null id is filtered by the query, not by the parser's patience.

        The workflow's `--jq` selects `.external_id != null`; this asserts the
        shape that selection produces, so a change to the query that let a
        null through is caught by the reader rather than by a live failure.
        """
        self.assertIsNone(liveness.posted_checks_from_api(0, '{"external_id": null, "id": 5}\n'))


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

    def test_an_update_does_not_resend_the_fixed_identity_fields(self) -> None:
        """A check run's name and head are fixed at creation.

        Resending them invites a rejection on a head that is not even wrong,
        and the failure would look like a permissions problem.
        """
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
        self.assertIn(HEAD, self._heads([finished]))

    def test_a_run_that_finished_long_ago_is_not_worth_reading(self) -> None:
        old = self._head(
            1, HEAD, status="completed", created_at="2026-09-19T00:00:00Z"
        )
        self.assertEqual(self._heads([old]), set())

    def test_the_bound_clears_the_slowest_observed_run(self) -> None:
        """The invariant that actually has to hold.

        `ripr`'s median is ~59 minutes and its slowest measured run 118. The
        bound is measured from `created_at`, so a bound shorter than a run's own
        lifetime would drop its head while the run was still going -- and a run
        that is still going is exactly what a retraction is for. Comfortably
        over the slowest observation, not merely over the median.
        """
        self.assertGreater(liveness.POSTED_MEMORY_HOURS * 60, 118 * 2)

    def test_the_post_completion_window_is_generous(self) -> None:
        """The bound is from `created_at`, so a long run gets less time after.

        For the slowest observed run the remaining window is still hours, which
        is many more 15-minute fires than a red needs to survive before the head
        is superseded anyway.
        """
        remaining = (liveness.POSTED_MEMORY_HOURS * 60) - 118
        self.assertGreaterEqual(remaining // 15, 8)

    def test_only_relevant_heads_are_selected_from_a_full_page(self) -> None:
        """The shape that actually happens: 100 runs, 92 heads, a few live."""
        runs: list[dict] = []
        for index in range(77):
            runs.append(
                self._head(
                    1000 + index,
                    f"{index:040x}",
                    status="completed",
                    created_at="2026-09-18T00:00:00Z",
                )
            )
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


if __name__ == "__main__":
    unittest.main()
