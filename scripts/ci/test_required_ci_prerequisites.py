#!/usr/bin/env python3
"""Exercise required-job scheduling and the pre-checkout prerequisite refusal."""

from __future__ import annotations

import ast
import copy
import os
from pathlib import Path
import re
import subprocess
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]
JOBS = ("check-all-targets", "conflict-markers")
CONTRACT_COMMAND = '"$RUNNER_TEMP/required-ci-regression/bin/python" scripts/ci/test_required_ci_prerequisites.py'


def workflow_jobs() -> dict:
    return yaml.safe_load((ROOT / ".github/workflows/ci.yml").read_text())["jobs"]


def context(guard="success", run_ci="true", preflight="success", latest="true",
            event="pull_request", draft=False) -> dict:
    return {
        "needs.draft-pr-check.result": guard,
        "needs.draft-pr-check.outputs.run_ci": run_ci,
        "needs.preflight-latest-check.result": preflight,
        "needs.preflight-latest-check.outputs.is_latest": latest,
        "github.event_name": event,
        "github.event.pull_request.draft": draft,
    }


def expression_value(expression: str, values: dict):
    """Finite Boolean grammar: reject anything outside these job predicates."""
    expression = expression.strip().removeprefix("${{").removesuffix("}}")
    expression = re.sub(r"\b(?:needs|github)\.[\w.-]+",
                        lambda m: repr(values[m[0]]), expression)
    expression = expression.replace("always()", "True")
    expression = re.sub(r"\btrue\b(?=(?:[^']*'[^']*')*[^']*$)", "True", expression)
    expression = re.sub(r"\bfalse\b(?=(?:[^']*'[^']*')*[^']*$)", "False", expression)
    tree = ast.parse(expression.replace("&&", " and ").replace("||", " or ").strip(),
                     mode="eval")

    def visit(node):
        if isinstance(node, ast.Constant):
            return node.value
        if isinstance(node, ast.BoolOp):
            operands = [bool(visit(value)) for value in node.values]
            if isinstance(node.op, ast.And):
                return all(operands)
            if isinstance(node.op, ast.Or):
                return any(operands)
        if isinstance(node, ast.Compare) and len(node.ops) == 1:
            left, right = visit(node.left), visit(node.comparators[0])
            # GitHub's expression oracle compares strings case-insensitively.
            if isinstance(left, str) and isinstance(right, str):
                left, right = left.casefold(), right.casefold()
            if isinstance(node.ops[0], ast.Eq):
                return left == right
            if isinstance(node.ops[0], ast.NotEq):
                return left != right
        raise AssertionError(f"unsupported workflow predicate: {ast.dump(node)}")

    return visit(tree.body)


def scheduled(job: dict, values: dict) -> bool:
    # GitHub implicitly prepends success() unless a status function occurs.
    condition = job["if"]
    needs_succeeded = all(values[f"needs.{need}.result"] == "success"
                          for need in job["needs"])
    return bool(expression_value(condition, values)) and (
        "always()" in condition or needs_succeeded
    )


def prerequisite_result(job: dict, values: dict) -> subprocess.CompletedProcess:
    step = job["steps"][0]
    if step.get("name") != "Validate CI prerequisites":
        raise AssertionError("prerequisite refusal must precede checkout and builds")
    if "if" in step or step.get("continue-on-error"):
        raise AssertionError("prerequisite refusal must be unconditional and required")
    env = dict(os.environ)
    env.update({name: str(expression_value(value, values))
                for name, value in step["env"].items()})
    return subprocess.run(["bash", "--noprofile", "--norc", "-euo", "pipefail", "-c",
                           step["run"]], env=env, capture_output=True, text=True,
                          timeout=10)


class RequiredPrerequisitesTests(unittest.TestCase):
    def setUp(self):
        self.jobs = workflow_jobs()

    def assert_refusal(self, values):
        for name in JOBS:
            with self.subTest(job=name, values=values):
                job = self.jobs[name]
                self.assertTrue(scheduled(job, values), "required job silently skipped")
                result = prerequisite_result(job, values)
                self.assertNotEqual(0, result.returncode, result.stdout + result.stderr)
                self.assertIn("::error::", result.stdout + result.stderr)

    def test_failed_guard_cannot_skip_required_jobs(self):
        for status in ("failure", "cancelled", "skipped", "", "unknown"):
            for run_ci, preflight, latest in (("true", "skipped", ""),
                                             ("false", "skipped", ""),
                                             ("", "skipped", ""),
                                             ("true", "success", "true")):
                self.assert_refusal(context(guard=status, run_ci=run_ci,
                                            preflight=preflight, latest=latest))

    def test_failed_preflight_cannot_skip_required_jobs(self):
        for status in ("failure", "cancelled", "skipped", "", "unknown"):
            for output in ("true", "false", ""):
                self.assert_refusal(context(preflight=status, latest=output))

    def test_malformed_success_outputs_fail_closed(self):
        for output in ("", " true ", "unknown"):
            self.assert_refusal(context(run_ci=output))
            self.assert_refusal(context(latest=output))

    def test_healthy_active_routes_keep_running(self):
        for event in ("pull_request", "merge_group", "push", "workflow_dispatch"):
            for name in JOBS:
                with self.subTest(event=event, job=name):
                    values = context(event=event)
                    job = self.jobs[name]
                    self.assertTrue(scheduled(job, values))
                    self.assertEqual(0, prerequisite_result(job, values).returncode)

    def test_draft_keeps_only_conflict_feedback(self):
        values = context(run_ci="false", preflight="skipped", latest="", draft=True)
        self.assertFalse(scheduled(self.jobs["check-all-targets"], values))
        job = self.jobs["conflict-markers"]
        self.assertTrue(scheduled(job, values))
        self.assertEqual(0, prerequisite_result(job, values).returncode)
        self.assert_refusal(dict(values, **{"needs.draft-pr-check.result": "failure"}))

    def test_healthy_unselected_and_superseded_routes_keep_skipping(self):
        for values in (context(run_ci="false", preflight="skipped", latest=""),
                       context(event="push", latest="false")):
            for name in JOBS:
                self.assertFalse(scheduled(self.jobs[name], values))

    def test_boolean_output_comparisons_match_github_case_insensitivity(self):
        for run_ci in ("true", "TRUE", "True"):
            for latest in ("true", "TRUE", "True"):
                for name in JOBS:
                    job = self.jobs[name]
                    values = context(run_ci=run_ci, latest=latest)
                    self.assertTrue(scheduled(job, values))
                    self.assertEqual(0, prerequisite_result(job, values).returncode)
        for false in ("false", "FALSE", "False"):
            for name in JOBS:
                self.assertFalse(scheduled(self.jobs[name], context(run_ci=false)))
                self.assertFalse(scheduled(self.jobs[name], context(latest=false)))

    def test_without_always_failed_needs_suppress_even_true_predicate(self):
        for name in JOBS:
            mutant = copy.deepcopy(self.jobs[name])
            mutant["if"] = mutant["if"].replace("always() &&", "")
            self.assertFalse(scheduled(mutant, context(guard="failure")))

    def test_swallowing_prerequisite_failure_is_detected(self):
        for name in JOBS:
            mutant = copy.deepcopy(self.jobs[name])
            mutant["steps"][0]["run"] = "true"
            self.assertEqual(0, prerequisite_result(mutant, context(guard="failure")).returncode)

    def test_existing_context_names_and_required_execution_are_retained(self):
        for name, check in (("check-all-targets", "Compile All Targets (bit-rot guard)"),
                            ("conflict-markers", "Conflict marker check")):
            self.assertEqual(check, self.jobs[name]["name"])
            self.assertNotIn("continue-on-error", self.jobs[name])
            self.assertEqual(["draft-pr-check", "preflight-latest-check"], self.jobs[name]["needs"])
        self.assertTrue(any(step.get("run") == CONTRACT_COMMAND
                            and "if" not in step and "continue-on-error" not in step
                            for step in self.jobs["check-all-targets"]["steps"]))


if __name__ == "__main__":
    unittest.main()
