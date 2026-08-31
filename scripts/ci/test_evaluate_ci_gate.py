#!/usr/bin/env python3
"""Focused falsifiers for the advisory CI Gate aggregate."""

from __future__ import annotations

import importlib.util
import io
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
