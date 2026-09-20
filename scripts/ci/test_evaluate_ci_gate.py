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
        for result in ("failure", "cancelled", "skipped", "neutral", "pending"):
            with self.subTest(result=result):
                verdict = gate.evaluate(applicable_needs(shard_result=result))
                self.assertEqual("failure", verdict.status)
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
        needs = applicable_needs()
        needs["future-required-input"] = {"result": "cancelled", "outputs": {}}
        verdict = gate.evaluate(needs)
        self.assertEqual("failure", verdict.status)
        self.assertEqual(("future-required-input=cancelled",), verdict.blockers)

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
        }
        environ.update(env)
        with mock.patch.dict(os.environ, environ, clear=True):
            with redirect_stdout(output):
                status = gate.main()
        return status, output.getvalue()

    def test_a_cancelled_run_that_a_newer_head_replaced_reports_green(self) -> None:
        """The #16087 case: a routine second push must not red the aggregate."""
        needs = applicable_needs(shard_result="cancelled")
        needs["check-all-targets"]["result"] = "cancelled"
        needs["ux-tests"]["result"] = "cancelled"

        verdict = gate.evaluate(
            needs, run_head=TESTED_HEAD, latest_head=NEWER_HEAD
        )
        self.assertEqual("superseded", verdict.status)

        status, summary = self._exit_status(
            needs, LATEST_HEAD_SHA=NEWER_HEAD
        )
        self.assertEqual(0, status)
        self.assertIn("superseded", summary)

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
        self.assertEqual("failure", verdict.status)

        # The exit code is the assertion that matters. #16107's rule and
        # #5460's before it: absent proof does not pass.
        status, _ = self._exit_status(needs, LATEST_HEAD_SHA=TESTED_HEAD)
        self.assertEqual(1, status)

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

    def test_a_cancelled_run_and_a_cancelled_lane_are_indistinguishable(
        self,
    ) -> None:
        """Why no verdict names cancellation as a cause.

        Run 35507473500 was concurrency-cancelled and still reported three
        green lanes, because the fast guards had already finished when the
        cancel landed. So a cancelled *run* and a single cancelled *job* reach
        this function as the same shape: some lanes cancelled, some green,
        none failed. Nothing in the needs map separates them, and the job's own
        `cancelled()` is false for both.

        Both are therefore `failure` — provable, and red either way. Only the
        head comparison distinguishes a state worth forgiving, which is the
        whole of the evidence model #16186's first attempt lacked.
        """
        lone_lane = applicable_needs(shard_result="cancelled")
        whole_run = _measured_cancelled_run()

        for label, needs in (("one job", lone_lane), ("whole run", whole_run)):
            with self.subTest(cancelled=label):
                verdict = gate.evaluate(
                    needs, run_head=TESTED_HEAD, latest_head=TESTED_HEAD
                )
                self.assertEqual("failure", verdict.status)

                status, _ = self._exit_status(
                    needs, LATEST_HEAD_SHA=TESTED_HEAD
                )
                self.assertEqual(1, status)

                # And the same two inputs are both forgiven once a newer head
                # is positively established. The head is doing all the work.
                superseded = gate.evaluate(
                    needs, run_head=TESTED_HEAD, latest_head=NEWER_HEAD
                )
                self.assertEqual("superseded", superseded.status)

    def test_a_real_failure_is_never_forgiven_however_far_the_head_moved(
        self,
    ) -> None:
        """Supersession forgives absent proof, never contrary proof.

        This is the assertion that keeps the head comparison from becoming a
        blanket amnesty: a dependency that genuinely failed is a failure even
        when the candidate it tested has been replaced.
        """
        needs = applicable_needs(shard_result="cancelled")
        needs["check-all-targets"]["result"] = "failure"

        verdict = gate.evaluate(
            needs, run_head=TESTED_HEAD, latest_head=NEWER_HEAD
        )
        self.assertEqual("failure", verdict.status)

        status, _ = self._exit_status(needs, LATEST_HEAD_SHA=NEWER_HEAD)
        self.assertEqual(1, status)

    def test_an_unrecognised_status_is_red(self) -> None:
        """A classification added later must not reach exit 0 by existing.

        The exit mapping is an allowlist of three statuses, not a denylist of
        failures, so a verdict nobody taught it about is red. A status
        added later cannot turn the gate green merely by existing.
        """
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
        must not read as a replacement.
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
                status, _ = self._exit_status(needs, LATEST_HEAD_SHA=value)
                self.assertEqual(1, status)

                # Asserted at the classifier too: the workflow's own
                # `^[0-9a-f]{40}$` guard must not be the only thing standing
                # between a garbled value and a green gate.
                self.assertFalse(gate._superseded(TESTED_HEAD, value))

        # The layer boundary, stated rather than assumed: `main` normalises
        # surrounding whitespace and `_superseded` judges the shape. A padded
        # object name is the same object name, so it is believed — the check
        # is for garbled values, not for tidy ones.
        padded = f"  {NEWER_HEAD}\n"
        self.assertFalse(gate._superseded(TESTED_HEAD, padded))
        status, _ = self._exit_status(needs, LATEST_HEAD_SHA=padded)
        self.assertEqual(0, status)

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
