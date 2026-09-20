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


TESTED_HEAD = "1111111111111111111111111111111111111111"
NEWER_HEAD = "2222222222222222222222222222222222222222"


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
            needs, run_cancelled=True, run_head=TESTED_HEAD, latest_head=NEWER_HEAD
        )
        self.assertEqual("superseded", verdict.status)

        status, summary = self._exit_status(
            needs, RUN_CANCELLED="true", LATEST_HEAD_SHA=NEWER_HEAD
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
        needs = applicable_needs(shard_result="cancelled")
        needs["check-all-targets"]["result"] = "cancelled"
        needs["ux-tests"]["result"] = "cancelled"

        verdict = gate.evaluate(
            needs, run_cancelled=True, run_head=TESTED_HEAD, latest_head=TESTED_HEAD
        )
        # Named rather than folded into `failure`: no dependency failed, and a
        # reader told one would go looking for something that is not there.
        self.assertEqual("cancelled_no_verdict", verdict.status)

        # The exit code is the assertion that matters. Naming the state must
        # not make it a pass, which is #16107's rule and #5460's before it.
        status, summary = self._exit_status(
            needs, RUN_CANCELLED="true", LATEST_HEAD_SHA=TESTED_HEAD
        )
        self.assertEqual(1, status)
        self.assertIn("cancelled_no_verdict", summary)

    def test_an_unresolved_live_head_stays_red(self) -> None:
        """No answer from the API is not evidence of a replacement."""
        needs = applicable_needs(shard_result="cancelled")
        needs["check-all-targets"]["result"] = "cancelled"
        needs["ux-tests"]["result"] = "cancelled"

        for latest in ("", "   "):
            with self.subTest(latest_head=latest):
                status, _ = self._exit_status(
                    needs, RUN_CANCELLED="true", LATEST_HEAD_SHA=latest
                )
                self.assertEqual(1, status)

    def test_a_cancelled_lane_in_a_live_run_stays_red(self) -> None:
        """A lane lost on its own produced no proof, and nothing superseded it.

        This is the case that must not be forgiven. A manual cancel, an API
        cancel or a runner dying leaves the blockers looking exactly like
        supersession, so only the run-level fact separates them. `ripr.yml`
        blocks the same shape as `cancelled-no-verdict` (#5460).
        """
        needs = applicable_needs(shard_result="cancelled")

        verdict = gate.evaluate(needs, run_cancelled=False)
        self.assertEqual("failure", verdict.status)

        status, _ = self._exit_status(needs)
        self.assertEqual(1, status)

    def test_a_real_failure_beside_a_cancellation_stays_red(self) -> None:
        needs = applicable_needs(shard_result="cancelled")
        needs["check-all-targets"]["result"] = "failure"

        verdict = gate.evaluate(needs, run_cancelled=True)
        self.assertEqual("failure", verdict.status)

        status, _ = self._exit_status(
            needs, RUN_CANCELLED="true", LATEST_HEAD_SHA=NEWER_HEAD
        )
        self.assertEqual(1, status)

    def test_an_unrecognised_status_is_red(self) -> None:
        """A classification added later must not reach exit 0 by existing.

        The exit mapping is an allowlist of three statuses, not a denylist of
        failures, so a verdict nobody taught it about is red. That is the
        property that lets `cancelled_no_verdict` be introduced at all.
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

    def test_only_the_exact_cancellation_marker_is_believed(self) -> None:
        """An unset or unexpected value must fail closed, not forgive."""
        needs = applicable_needs(shard_result="cancelled")
        needs["check-all-targets"]["result"] = "cancelled"
        needs["ux-tests"]["result"] = "cancelled"

        for value in ("", "false", "True", "TRUE", "1", "cancelled"):
            with self.subTest(run_cancelled=value):
                status, _ = self._exit_status(
                    needs, RUN_CANCELLED=value, LATEST_HEAD_SHA=NEWER_HEAD
                )
                self.assertEqual(1, status)

    def test_workflow_records_cancellation_before_the_classifier_reads_it(self) -> None:
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        start = workflow.index("  merge-gate:\n")
        end = workflow.index("\n  # \u2500\u2500 UX Tests", start)
        block = workflow[start:end]

        self.assertIn("name: Record run cancellation", block)
        self.assertIn("python3 scripts/ci/evaluate_ci_gate.py", block)
        record = block.index("name: Record run cancellation")
        classifier = block.index("python3 scripts/ci/evaluate_ci_gate.py")
        self.assertLess(
            record,
            classifier,
            "the cancellation marker must be exported before the step that reads it",
        )
        self.assertIn("if: cancelled()", block)
        self.assertIn('RUN_CANCELLED=true" >> "$GITHUB_ENV"', block)

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
