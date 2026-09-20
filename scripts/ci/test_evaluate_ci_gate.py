#!/usr/bin/env python3
"""Focused falsifiers for the advisory CI Gate aggregate."""

from __future__ import annotations

import importlib.util
import io
import json
import os
import re
import sys
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

SCRIPT = Path(__file__).with_name("evaluate_ci_gate.py")
SPEC = importlib.util.spec_from_file_location("evaluate_ci_gate", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load aggregate evaluator from {SCRIPT}")
gate = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = gate
SPEC.loader.exec_module(gate)

ROOT = Path(__file__).resolve().parents[2]


# Real object names, not repeated digits: a fixture of "1" * 40 is equal
# to its own `.upper()`, which silently makes a case assertion vacuous.
TESTED_HEAD = "2a4f5bcf1d0e7c9b8a6f5e4d3c2b1a0918273645"
NEWER_HEAD = "cc291563b7a0e5d4f3c2b1a09182736455647382"
# The replacement run's id. A moved head says the candidate changed; this
# says something exists to prove the new one, which is what #16087 asks for.
REPLACEMENT_RUN = "35508361576"


def applicable_needs(*, shard_result: str = "success") -> dict[str, dict]:
    return {
        "draft-pr-check": {
            "result": "success",
            "outputs": {"run_ci": "true"},
        },
        "preflight-latest-check": {
            "result": "success",
            "outputs": {"is_latest": "true"},
        },
        "conflict-markers": {"result": "success", "outputs": {}},
        "check-all-targets": {"result": "success", "outputs": {}},
        "ux-tests": {"result": "success", "outputs": {}},
        "merge-gate-shards": {"result": shard_result, "outputs": {}},
    }


def _merge_gate_block() -> str:
    """The `merge-gate` job's text from the real workflow file.

    Same slice the workflow-hardening tests below take. Extracting it by the
    job's own boundaries rather than by searching for a step name is what
    keeps an edit to a *different* job from silently satisfying these
    assertions.
    """
    workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    start = workflow.index("  merge-gate:\n")
    end = workflow.index("\n  # \u2500\u2500 UX Tests", start)
    return workflow[start:end]


def _measured_cancelled_run() -> dict[str, dict]:
    """The needs map of run 35507473500, a real concurrency cancellation.

    Fifteen lanes cancelled; `Draft PR guard`, `Preflight` and `Conflict
    marker check` green, having finished before the cancel landed. That
    survivor set is why "every lane cancelled" is not the shape of a
    cancelled run, and why the early guards do not trip on one.
    """
    needs = applicable_needs(shard_result="cancelled")
    needs["check-all-targets"]["result"] = "cancelled"
    needs["ux-tests"]["result"] = "cancelled"
    return needs


class AggregateClassifierTests(unittest.TestCase):
    def test_all_applicable_dependencies_must_succeed(self) -> None:
        verdict = gate.evaluate(applicable_needs())
        self.assertEqual("success", verdict.status)
        self.assertEqual((), verdict.blockers)

    def test_non_success_shard_cannot_report_green(self) -> None:
        """No non-success result reaches the green allowlist, whatever it is.

        #16087 asked for the outcomes to be partitioned: a lane that reached a
        verdict and lost is `failure`, a lane cancelled before it could reach
        one is `no_verdict`. The label therefore differs by result, and the
        exact label is still pinned below so it cannot drift silently. What
        the test is *named* for is the other assertion — membership of
        `GREEN_STATUSES`, the set `main` actually reads — and that is
        unchanged for every input: none of them is green.
        """
        for result in ("failure", "cancelled", "skipped", "neutral", "pending"):
            with self.subTest(result=result):
                verdict = gate.evaluate(applicable_needs(shard_result=result))
                self.assertEqual(
                    "no_verdict" if result == "cancelled" else "failure",
                    verdict.status,
                )
                self.assertNotIn(verdict.status, gate.GREEN_STATUSES)
                self.assertEqual(
                    (f"merge-gate-shards={result}",),
                    verdict.blockers,
                )

    def test_missing_shard_cannot_report_green(self) -> None:
        needs = applicable_needs()
        del needs["merge-gate-shards"]
        verdict = gate.evaluate(needs)
        self.assertEqual("failure", verdict.status)
        self.assertEqual(("merge-gate-shards=missing",), verdict.blockers)

    def test_malformed_needs_json_cannot_report_green(self) -> None:
        output = io.StringIO()
        with mock.patch.dict(os.environ, {"NEEDS_JSON": "not-json"}, clear=True):
            with redirect_stdout(output):
                status = gate.main()
        self.assertEqual(1, status)
        self.assertIn("aggregate input was malformed", output.getvalue())

    def test_new_non_success_dependency_cannot_be_ignored(self) -> None:
        """A dependency nobody anticipated is still named and still red.

        Covered on both sides of the partition. The cancelled case is the one
        that changed label, and it is the one that could plausibly be
        swallowed by the new branch, so a failed case rides alongside it to
        prove the branch narrows nothing.
        """
        for result, expected in (("cancelled", "no_verdict"), ("failure", "failure")):
            with self.subTest(result=result):
                needs = applicable_needs()
                needs["future-required-input"] = {"result": result, "outputs": {}}
                verdict = gate.evaluate(needs)
                self.assertEqual(expected, verdict.status)
                self.assertNotIn(verdict.status, gate.GREEN_STATUSES)
                self.assertEqual(
                    (f"future-required-input={result}",), verdict.blockers
                )

    def test_failed_draft_guard_cannot_be_misread_as_scoped_noop(self) -> None:
        needs = applicable_needs()
        needs["draft-pr-check"] = {
            "result": "failure",
            "outputs": {"run_ci": "false"},
        }
        verdict = gate.evaluate(needs)
        self.assertEqual("failure", verdict.status)
        self.assertEqual(("draft-pr-check=failure",), verdict.blockers)

    def test_skipped_preflight_on_applicable_route_is_failure(self) -> None:
        needs = applicable_needs()
        needs["preflight-latest-check"] = {
            "result": "skipped",
            "outputs": {},
        }
        verdict = gate.evaluate(needs)
        self.assertEqual("failure", verdict.status)
        self.assertEqual(("preflight-latest-check=skipped",), verdict.blockers)

    def test_draft_route_is_positive_scoped_noop(self) -> None:
        needs = applicable_needs(shard_result="skipped")
        needs["draft-pr-check"]["outputs"]["run_ci"] = "false"
        needs["preflight-latest-check"] = {"result": "skipped", "outputs": {}}
        verdict = gate.evaluate(
            needs,
            event_name="pull_request",
            pull_request_draft="true",
        )
        self.assertEqual("scoped_noop", verdict.status)
        self.assertEqual("draft pull request", verdict.reason)

    def test_draft_scoped_noop_fails_on_failure_cancelled_or_unknown_skip(self) -> None:
        for name, result in (
            ("conflict-markers", "failure"),
            ("check-all-targets", "cancelled"),
            ("future-required-input", "skipped"),
        ):
            with self.subTest(name=name, result=result):
                needs = applicable_needs(shard_result="skipped")
                needs["draft-pr-check"]["outputs"]["run_ci"] = "false"
                needs["preflight-latest-check"] = {"result": "skipped", "outputs": {}}
                needs[name] = {"result": result, "outputs": {}}
                verdict = gate.evaluate(
                    needs,
                    event_name="pull_request",
                    pull_request_draft="true",
                )
                self.assertEqual("failure", verdict.status)
                self.assertIn(f"{name}={result}", verdict.blockers)

    def test_non_draft_route_cannot_claim_draft_scoped_noop(self) -> None:
        needs = applicable_needs(shard_result="skipped")
        needs["draft-pr-check"]["outputs"]["run_ci"] = "false"
        verdict = gate.evaluate(
            needs,
            event_name="pull_request",
            pull_request_draft="false",
        )
        self.assertEqual("failure", verdict.status)
        self.assertEqual(("draft-pr-check.run_ci=false",), verdict.blockers)

    def test_superseded_push_is_positive_scoped_noop(self) -> None:
        needs = applicable_needs(shard_result="skipped")
        needs["preflight-latest-check"]["outputs"]["is_latest"] = "false"
        verdict = gate.evaluate(needs, event_name="push")
        self.assertEqual("scoped_noop", verdict.status)
        self.assertEqual("superseded push", verdict.reason)

    def test_non_push_route_cannot_claim_superseded_scoped_noop(self) -> None:
        needs = applicable_needs(shard_result="skipped")
        needs["preflight-latest-check"]["outputs"]["is_latest"] = "false"
        verdict = gate.evaluate(needs, event_name="merge_group")
        self.assertEqual("failure", verdict.status)
        self.assertEqual(
            ("preflight-latest-check.is_latest=false",),
            verdict.blockers,
        )

    def test_superseded_scoped_noop_fails_on_downstream_failure(self) -> None:
        needs = applicable_needs(shard_result="skipped")
        needs["preflight-latest-check"]["outputs"]["is_latest"] = "false"
        needs["merge-gate-shards"] = {"result": "failure", "outputs": {}}
        verdict = gate.evaluate(needs, event_name="push")
        self.assertEqual("failure", verdict.status)
        self.assertEqual(("merge-gate-shards=failure",), verdict.blockers)


class AggregateWiringTests(unittest.TestCase):
    def _exit_status(self, needs: dict, **env: str) -> tuple[int, str]:
        """Drive `main()` end to end and return its exit code and summary.

        The exit code is the whole interface between this classifier and the
        check's colour: the aggregate job's only step is
        `python3 scripts/ci/evaluate_ci_gate.py`. A test that asserts on
        `verdict.status` alone passes identically whether or not that status
        ever reaches GitHub, so every claim about red or green is made here.
        """
        output = io.StringIO()
        environ = {
            "NEEDS_JSON": json.dumps(needs),
            "EVENT_NAME": "pull_request",
            "RUN_HEAD_SHA": TESTED_HEAD,
            "REPLACEMENT_RUN_ID": REPLACEMENT_RUN,
        }
        environ.update(env)
        with mock.patch.dict(os.environ, environ, clear=True):
            with redirect_stdout(output):
                status = gate.main()
        return status, output.getvalue()

    def test_the_16087_case_stays_red_but_says_why(self) -> None:
        """The #16087 case: red is right, the sentence was the defect.

        The issue's own Proposed fix settles the colour — "exiting 1 keeps a
        superseded head visibly unproven, which is honest... my
        recommendation is exit 1 with the distinguishing message". So this
        asserts the message, because with every non-success path exiting 1
        the exit code can no longer tell these cases apart, and a test that
        only checked the code would pass whatever the reader is told.
        """
        needs = _measured_cancelled_run()

        verdict = gate.evaluate(
            needs,
            run_head=TESTED_HEAD,
            latest_head=NEWER_HEAD,
            replacement_run=REPLACEMENT_RUN,
        )
        self.assertEqual("superseded", verdict.status)

        status, summary = self._exit_status(needs, LATEST_HEAD_SHA=NEWER_HEAD)
        self.assertEqual(1, status)

        # The whole point: a reader must be able to tell this from a broken
        # test without opening the run.
        self.assertIn("NOT a test failure", summary)
        self.assertIn(NEWER_HEAD[:8], summary)
        self.assertIn(REPLACEMENT_RUN, summary)
        self.assertNotIn("applicable dependency did not succeed", summary)

    def test_a_real_failure_still_gets_the_old_sentence(self) -> None:
        """The discriminator, from the other side.

        Every assertion above would pass against a classifier that printed
        "NOT a test failure" unconditionally. This is what stops that: a lane
        that genuinely failed must still read as a failure, and must not be
        handed the reassuring wording.
        """
        needs = _measured_cancelled_run()
        needs["check-all-targets"]["result"] = "failure"

        status, summary = self._exit_status(needs, LATEST_HEAD_SHA=NEWER_HEAD)
        self.assertEqual(1, status)
        self.assertIn("applicable dependency did not succeed", summary)
        self.assertNotIn("NOT a test failure", summary)

    def test_a_hand_cancelled_run_with_no_newer_head_stays_red(self) -> None:
        """A maintainer cancelling a run is not supersession.

        `cancelled()` reports that a run was cancelled, not why, so this input
        is indistinguishable from #16087's at the run level. Only the live head
        separates them, and here it has not moved: nothing replaced this
        candidate, so nothing is proving it and the gate must stay red. This is
        the case #5460 refused a pass verdict for, and it still refuses.
        """
        needs = _measured_cancelled_run()

        verdict = gate.evaluate(
            needs, run_head=TESTED_HEAD, latest_head=TESTED_HEAD
        )
        self.assertEqual("no_verdict", verdict.status)
        self.assertNotIn(verdict.status, gate.GREEN_STATUSES)

        # The exit code is the assertion that matters. #16107's rule and
        # #5460's before it: absent proof does not pass.
        status, summary = self._exit_status(needs, LATEST_HEAD_SHA=TESTED_HEAD)
        self.assertEqual(1, status)

        # And it must not name a replacement it has not got. The sentence is
        # the whole of #16087's complaint, so a red that says the wrong thing
        # is still the defect.
        self.assertIn("no newer run was identified", summary)
        self.assertNotIn(REPLACEMENT_RUN, summary)

    def test_an_unresolved_live_head_stays_red(self) -> None:
        """No answer from the API is not evidence of a replacement."""
        needs = applicable_needs(shard_result="cancelled")
        needs["check-all-targets"]["result"] = "cancelled"
        needs["ux-tests"]["result"] = "cancelled"

        for latest in ("", "   "):
            with self.subTest(latest_head=latest):
                status, _ = self._exit_status(
                    needs, LATEST_HEAD_SHA=latest
                )
                self.assertEqual(1, status)

    def test_a_lone_cancelled_lane_and_a_cancelled_run_are_indistinguishable(
        self,
    ) -> None:
        """The classifier cannot tell them apart, so it must not claim to.

        Review raised the lone cancelled lane as a gap: a blocker-shaped
        predicate cannot tell a superseded run from one lost lane. That is
        correct, and it is not repairable at this layer. Run 35507473500 was
        concurrency-cancelled and still reported three green lanes, because
        the fast guards finished before the cancel landed — so a whole
        cancelled run and a single cancelled lane genuinely arrive here as
        the same shape, and the job's own `cancelled()` is false for both.

        Under exit 1 the gap costs a sentence rather than a pass, which is
        why the assertions below are about sentences. Both inputs are red
        either way. What the superseded wording may say is limited to what
        the inputs establish: this run tested a head that is no longer the
        candidate, and a run of this workflow exists for the one that is.
        Both facts come from the API, neither is a claim about why a lane
        died, and the wording must not imply one — naming a cause it cannot
        establish is precisely the defect #16087 is about.
        """
        lone_lane = applicable_needs(shard_result="cancelled")
        whole_run = _measured_cancelled_run()

        for label, needs in (("one job", lone_lane), ("whole run", whole_run)):
            with self.subTest(cancelled=label):
                verdict = gate.evaluate(
                    needs, run_head=TESTED_HEAD, latest_head=TESTED_HEAD
                )
                self.assertEqual("no_verdict", verdict.status)

                status, _ = self._exit_status(
                    needs, LATEST_HEAD_SHA=TESTED_HEAD
                )
                self.assertEqual(1, status)

                # With the replacement bound, both take the superseded
                # sentence — the two shapes are alike here precisely because
                # nothing can separate them, and the sentence is chosen by
                # evidence about the *subject*, not about the lanes.
                superseded = gate.evaluate(
                    needs,
                    run_head=TESTED_HEAD,
                    latest_head=NEWER_HEAD,
                    replacement_run=REPLACEMENT_RUN,
                )
                self.assertEqual("superseded", superseded.status)
                self.assertNotIn(superseded.status, gate.GREEN_STATUSES)

                # The wording is bounded by the evidence. It may name the
                # newer head and the run found for it; it may not say the
                # cancellation was caused by the push, which no input shows.
                self.assertIn(NEWER_HEAD[:8], superseded.reason)
                self.assertIn(REPLACEMENT_RUN, superseded.reason)
                for unprovable in ("because", "superseded by", "second push"):
                    self.assertNotIn(unprovable, superseded.reason)

    def test_a_moved_head_with_no_replacement_run_stays_red(self) -> None:
        """Head movement is not replacement evidence. The review's finding.

        A push moves the pull request's head, but nothing guarantees a run
        started for it: the workflow may be filtered out on the new head,
        Actions may be degraded, the lookup may fail, or the push may simply
        be newer than any scheduling. In all of those the candidate changed
        and *nothing* is proving the new one, so "superseded" would claim a
        replacement that does not exist. #16087 asks for a demonstrable newer
        run; without one this is absent proof and stays red.
        """
        needs = _measured_cancelled_run()

        verdict = gate.evaluate(
            needs,
            run_head=TESTED_HEAD,
            latest_head=NEWER_HEAD,
            replacement_run="",
        )
        self.assertEqual("no_verdict", verdict.status)
        self.assertNotIn(verdict.status, gate.GREEN_STATUSES)

        status, summary = self._exit_status(
            needs, LATEST_HEAD_SHA=NEWER_HEAD, REPLACEMENT_RUN_ID=""
        )
        self.assertEqual(1, status)

        # The finding restated as an assertion: with the head moved and no
        # run bound, the summary must claim no replacement and must not name
        # the newer head as though something were proving it.
        self.assertIn("no newer run was identified", summary)
        self.assertNotIn(NEWER_HEAD[:8], summary)

    def test_only_a_well_formed_replacement_run_id_is_believed(self) -> None:
        """A garbled replacement id is unknown, never a replacement.

        The workflow step exports the id only when it matches `^[0-9]+$`, but
        that shell condition is one layer, and this is the input that selects
        the superseded sentence. Anything that is not plainly a run id must
        read as "no replacement found", because a message pointing at a run
        that is not proving this candidate is worse than the generic wording
        it replaces.
        """
        needs = _measured_cancelled_run()
        malformed = (
            "",
            "   ",
            "null",  # jq's output when the field is absent and `// empty` is lost
            "empty",
            "0x1234",
            "35508361576abc",
            "-35508361576",
            "35508361576 35508361577",
        )
        for value in malformed:
            with self.subTest(replacement_run=value):
                self.assertFalse(
                    gate._superseded(TESTED_HEAD, NEWER_HEAD, value)
                )
                status, summary = self._exit_status(
                    needs,
                    LATEST_HEAD_SHA=NEWER_HEAD,
                    REPLACEMENT_RUN_ID=value,
                )
                self.assertEqual(1, status)
                self.assertIn("no newer run was identified", summary)
                self.assertNotIn(NEWER_HEAD[:8], summary)

    def test_the_workflow_binds_the_replacement_run_within_this_workflow(
        self,
    ) -> None:
        """The lookup must be scoped, and must not find this run itself.

        Two properties of the resolution step that no unit test of the
        classifier can reach, because they live in the shell. The lookup is
        keyed on the run's own `workflow_id`, so a run of some *other*
        workflow on the newer head cannot be mistaken for the replacement;
        and it is guarded on the heads already differing, so a run can never
        return itself as its own replacement.
        """
        block = _merge_gate_block()
        self.assertIn("workflow_id", block)
        self.assertIn("actions/workflows/${workflow_id}/runs?head_sha=", block)
        self.assertIn('"${latest}" != "${tested}"', block)
        self.assertIn('"${replacement}" =~ ^[0-9]+$', block)
        # Exported exactly once, and only after the shape check. A second
        # unguarded export anywhere in the job would defeat the check.
        self.assertEqual(1, block.count("REPLACEMENT_RUN_ID="))
        self.assertLess(
            block.index('"${replacement}" =~ ^[0-9]+$'),
            block.index("REPLACEMENT_RUN_ID="),
        )

    def test_a_real_failure_keeps_its_own_sentence_however_far_the_head_moved(
        self,
    ) -> None:
        """The superseded wording covers absent proof, never contrary proof.

        Every path is red, so what the head comparison can still get wrong is
        the sentence, and this is the case where the wrong sentence would be
        most costly: a dependency that genuinely failed, told to the reader as
        "this is NOT a test failure" because the candidate moved on. A lane
        that reached a verdict and lost is information about the code, and
        code survives a head move in a way a cancellation does not.
        """
        needs = applicable_needs(shard_result="cancelled")
        needs["check-all-targets"]["result"] = "failure"

        verdict = gate.evaluate(
            needs,
            run_head=TESTED_HEAD,
            latest_head=NEWER_HEAD,
            replacement_run=REPLACEMENT_RUN,
        )
        self.assertEqual("failure", verdict.status)
        self.assertNotIn("NOT a test failure", verdict.reason)

        status, summary = self._exit_status(
            needs, LATEST_HEAD_SHA=NEWER_HEAD
        )
        self.assertEqual(1, status)
        self.assertIn("applicable dependency did not succeed", summary)
        self.assertNotIn("NOT a test failure", summary)

    def test_an_unrecognised_status_is_red(self) -> None:
        """A classification added later must not reach exit 0 by existing.

        The exit mapping is an allowlist of two statuses, not a denylist of
        failures, so a verdict nobody taught it about is red. A status added
        later cannot turn the gate green merely by existing.
        """
        self.assertEqual({"success", "scoped_noop"}, set(gate.GREEN_STATUSES))

        output = io.StringIO()
        with mock.patch.object(
            gate, "evaluate", return_value=gate.Verdict("a_new_idea", "whatever")
        ):
            with mock.patch.dict(
                os.environ,
                {"NEEDS_JSON": json.dumps(applicable_needs()), "EVENT_NAME": "pull_request"},
                clear=True,
            ):
                with redirect_stdout(output):
                    self.assertEqual(1, gate.main())

    def test_only_a_well_formed_differing_head_is_believed(self) -> None:
        """Fail closed on anything that is not plainly a newer object name.

        This replaces a test of the `RUN_CANCELLED` marker's exact spelling.
        That marker is gone, so the input that can now be malformed is the
        live head, and the same discipline applies to it: a value the
        workflow would not have exported, or one equal to the tested head,
        must not read as a replacement. Nothing here turns the gate green —
        both outcomes exit 1 — so what a wrong answer costs is a message
        naming a replacement that does not exist, which is the defect this
        change exists to remove rather than reproduce.
        """
        needs = applicable_needs(shard_result="cancelled")
        needs["check-all-targets"]["result"] = "cancelled"
        needs["ux-tests"]["result"] = "cancelled"

        malformed = (
            "",
            "   ",
            TESTED_HEAD,  # not a replacement at all
            TESTED_HEAD.upper(),  # the same object, differently spelled
            NEWER_HEAD[:39],  # truncated: unknown, not newer
            NEWER_HEAD + "0",  # over-long
            NEWER_HEAD[:-1] + "g",  # not hexadecimal
        )
        for value in malformed:
            with self.subTest(latest_head=value):
                status, summary = self._exit_status(needs, LATEST_HEAD_SHA=value)
                self.assertEqual(1, status)
                self.assertIn("no newer run was identified", summary)
                self.assertNotIn(REPLACEMENT_RUN, summary)

                # Asserted at the classifier too: the workflow's own
                # `^[0-9a-f]{40}$` guard must not be the only thing standing
                # between a garbled value and a false claim of supersession.
                self.assertFalse(
                    gate._superseded(TESTED_HEAD, value, REPLACEMENT_RUN)
                )

        # The layer boundary, stated rather than assumed: `main` normalises
        # surrounding whitespace and `_superseded` judges the shape. A padded
        # object name is the same object name, so it is believed — the check
        # is for garbled values, not for tidy ones.
        #
        # The exit code cannot show that any more, because every path exits 1.
        # The sentence can: a believed head is named in it, a rejected one is
        # not. That is the only observable difference the two now have, and it
        # is the one #16087 is about.
        padded = f"  {NEWER_HEAD}\n"
        self.assertFalse(gate._superseded(TESTED_HEAD, padded, REPLACEMENT_RUN))
        status, summary = self._exit_status(needs, LATEST_HEAD_SHA=padded)
        self.assertEqual(1, status)
        self.assertIn(NEWER_HEAD[:8], summary)
        self.assertIn(REPLACEMENT_RUN, summary)

    def test_the_head_resolution_is_not_gated_on_cancelled(self) -> None:
        """The signal must reach the classifier in a cancelled run.

        This replaces a test that pinned the ordering of an `if: cancelled()`
        marker step. Run 35507473500 falsified that design: the run was
        concurrency-cancelled, every dependency came back `cancelled`, and
        this job's `if: cancelled()` steps were skipped. `cancelled()` is
        false here, so anything behind it could never fire in the one case
        #16087 is about.

        The heads are therefore resolved under `always()`, and no step in
        this job may reintroduce a `cancelled()` guard on the path that
        feeds the classifier.
        """
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        start = workflow.index("  merge-gate:\n")
        end = workflow.index("\n  # \u2500\u2500 UX Tests", start)
        block = workflow[start:end]

        resolve = block.index("name: Resolve the tested head")
        guard = block.index("if:", resolve)
        self.assertIn("always()", block[guard : guard + 60])

        # Comments are stripped: this job's own comment explains the
        # `cancelled()` finding and quotes the directive it removed, and an
        # assertion that tripped over that prose would be checking the wrong
        # thing. What must not come back is a live guard.
        directives = "\n".join(
            line for line in block.splitlines() if not line.lstrip().startswith("#")
        )
        self.assertNotIn("cancelled()", directives)
        self.assertNotIn("RUN_CANCELLED", directives)

    def test_workflow_resolves_both_heads_from_the_api(self) -> None:
        """The comparison is only as trustworthy as where the two heads come from.

        The classifier cannot tell a real supersession from a fabricated one,
        so the job must read both sides from the API keyed on `github.run_id`,
        which the candidate cannot set. Taking either head from the event
        payload would let the compared values travel with the branch.
        """
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        start = workflow.index("  merge-gate:\n")
        end = workflow.index("\n  # \u2500\u2500 UX Tests", start)
        block = workflow[start:end]

        self.assertIn("RUN_HEAD_SHA=", block)
        self.assertIn("LATEST_HEAD_SHA=", block)
        self.assertIn("actions/runs/${RUN_ID}", block)
        self.assertIn("RUN_ID: ${{ github.run_id }}", block)

        resolve = block.index("RUN_HEAD_SHA=")
        classifier = block.index("python3 scripts/ci/evaluate_ci_gate.py")
        self.assertLess(
            resolve,
            classifier,
            "both heads must be exported before the step that compares them",
        )

    def test_workflow_uses_one_unconditional_job_check(self) -> None:
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        start = workflow.index("  merge-gate:\n")
        end = workflow.index("\n  # \u2500\u2500 UX Tests", start)
        block = workflow[start:end]

        self.assertIn("name: CI Gate (Advisory Aggregate)", block)
        self.assertIn("if: always()", block)
        self.assertIn("python3 scripts/ci/evaluate_ci_gate.py", block)
        self.assertNotIn("github.event.pull_request.head.sha", block)
        self.assertNotIn("statuses: write", block)
        self.assertNotIn("ci/merge-gate", block)

        needs_match = re.search(
            r"\n    needs:\n(?P<items>(?:      - [^\n]+\n)+)",
            block,
        )
        self.assertIsNotNone(needs_match)
        actual = tuple(
            line.strip()[2:]
            for line in needs_match.group("items").splitlines()
        )
        self.assertEqual(gate.EXPECTED_DEPENDENCIES, actual)

    def test_policy_records_advisory_hardening_without_claiming_live_enforcement(self) -> None:
        policy = (ROOT / ".ci/policies/required-checks.toml").read_text(
            encoding="utf-8"
        )
        start = policy.index('name = "CI Gate (Advisory Aggregate)"')
        end = policy.index("\n[[checks]]", start)
        row = policy[start:end]

        self.assertIn('applicability = "always-or-scoped-noop"', row)
        self.assertIn("required = false", row)
        self.assertIn('policy_role = "advisory"', row)
        self.assertIn('enforcement = "neither"', row)
        self.assertIn("#12911", row)
        self.assertNotIn("required-promotion", row)


if __name__ == "__main__":
    unittest.main()
