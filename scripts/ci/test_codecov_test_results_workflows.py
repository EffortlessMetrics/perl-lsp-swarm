#!/usr/bin/env python3
"""Structural contract for the four advisory Codecov test-result uploads."""

from __future__ import annotations

import unittest
from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[2]
ACTION = "codecov/codecov-action@0fb7174895f61a3b6b78fc075e0cd60383518dac"
TOKEN = "${{ secrets.CODECOV_TOKEN }}"

EXPECTED = {
    "Upload PR-fast test results to Codecov": {
        "file": ".github/workflows/ci.yml",
        "if": "always() && env.CODECOV_TOKEN != ''",
        "files": "target/test-results/pr-fast-junit.xml",
        "token": TOKEN,
        "env_token": TOKEN,
    },
    "Upload gate shard test results to Codecov": {
        "file": ".github/workflows/ci.yml",
        "if": "always() && env.CODECOV_TOKEN != ''",
        "files": "target/test-results/gate-shard-${{ matrix.name }}-junit.xml",
        "token": TOKEN,
        "env_token": TOKEN,
    },
    "Upload UX regression test results to Codecov": {
        "file": ".github/workflows/ci.yml",
        "if": "always() && env.CODECOV_TOKEN != ''",
        "files": "target/test-results/ux-regression-junit.xml",
        "token": TOKEN,
        "env_token": TOKEN,
    },
    "Upload test results to Codecov without a repository secret": {
        "file": ".github/workflows/ux-regression-gate.yml",
        "if": "always() && steps.harness_check.outputs.harness_available == 'true'",
        "files": "target/test-results/ux-regression-gate-junit.xml",
        "token": None,
        "env_token": None,
    },
}


def load_workflows() -> dict[str, dict]:
    return {
        path: yaml.load((ROOT / path).read_text(encoding="utf-8"), Loader=yaml.BaseLoader)
        for path in {entry["file"] for entry in EXPECTED.values()}
    }


def raw_steps(workflow: dict) -> list[dict]:
    found = []
    for job in workflow.get("jobs", {}).values():
        for step in job.get("steps", []):
            if isinstance(step, dict):
                found.append(step)
    return found


def validate(workflows: dict[str, dict]) -> None:
    all_steps = {path: raw_steps(data) for path, data in workflows.items()}
    for steps in all_steps.values():
        for step in steps:
            if str(step.get("uses", "")).startswith("codecov/test-results-action@"):
                raise AssertionError("deprecated Codecov test-results action remains")

    population = [
        (path, step.get("name"))
        for path, steps in all_steps.items()
        for step in steps
        if step.get("uses") == ACTION
        and step.get("with", {}).get("report_type") == "test_results"
    ]
    expected_population = [(entry["file"], name) for name, entry in EXPECTED.items()]
    if sorted(population) != sorted(expected_population):
        raise AssertionError("Codecov test-result upload population drifted")

    for name, expected in EXPECTED.items():
        matches = [step for step in all_steps[expected["file"]] if step.get("name") == name]
        if len(matches) != 1:
            raise AssertionError(f"missing named upload step: {name}")
        step = matches[0]
        if step.get("uses") != ACTION:
            raise AssertionError(f"{name}: action pin drifted")
        if step.get("if") != expected["if"]:
            raise AssertionError(f"{name}: condition drifted")
        if step.get("continue-on-error") != "true":
            raise AssertionError(f"{name}: advisory failure posture drifted")
        inputs = step.get("with", {})
        if inputs.get("report_type") != "test_results":
            raise AssertionError(f"{name}: report_type drifted")
        if inputs.get("files") != expected["files"]:
            raise AssertionError(f"{name}: result file drifted")
        if inputs.get("token") != expected["token"]:
            raise AssertionError(f"{name}: token input drifted")
        if step.get("env", {}).get("CODECOV_TOKEN") != expected["env_token"]:
            raise AssertionError(f"{name}: token environment drifted")
        fail_value = inputs.get("fail_ci_if_error")
        if expected["token"] is None:
            if fail_value != "false":
                raise AssertionError(f"{name}: tokenless failure flag drifted")
        elif fail_value is not None:
            raise AssertionError(f"{name}: unexpected failure flag")


class CodecovTestResultsWorkflowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflows = load_workflows()

    def mutate(self, name: str, key: str, value: object, section: str | None = None) -> None:
        steps = raw_steps(self.workflows[EXPECTED[name]["file"]])
        step = next(step for step in steps if step.get("name") == name)
        target = step if section is None else step.setdefault(section, {})
        target[key] = value

    def assert_rejected(self) -> None:
        with self.assertRaises(AssertionError):
            validate(self.workflows)

    def test_current_workflows_match_contract(self) -> None:
        validate(self.workflows)

    def test_rejects_old_or_wrong_action(self) -> None:
        self.mutate("Upload PR-fast test results to Codecov", "uses", "codecov/test-results-action@old")
        self.assert_rejected()

    def test_rejects_missing_report_type(self) -> None:
        self.mutate("Upload gate shard test results to Codecov", "report_type", "coverage", "with")
        self.assert_rejected()

    def test_rejects_wrong_result_file(self) -> None:
        self.mutate("Upload UX regression test results to Codecov", "files", "decoy.xml", "with")
        self.assert_rejected()

    def test_rejects_lost_always_condition(self) -> None:
        self.mutate("Upload PR-fast test results to Codecov", "if", "env.CODECOV_TOKEN != ''")
        self.assert_rejected()

    def test_rejects_lost_advisory_posture(self) -> None:
        self.mutate("Upload gate shard test results to Codecov", "continue-on-error", "false")
        self.assert_rejected()

    def test_rejects_secret_backed_token_drift(self) -> None:
        self.mutate("Upload UX regression test results to Codecov", "token", "", "with")
        self.assert_rejected()

    def test_rejects_token_added_to_tokenless_upload(self) -> None:
        name = "Upload test results to Codecov without a repository secret"
        self.mutate(name, "token", TOKEN, "with")
        self.assert_rejected()

    def test_rejects_tokenless_failure_flag_drift(self) -> None:
        name = "Upload test results to Codecov without a repository secret"
        self.mutate(name, "fail_ci_if_error", "true", "with")
        self.assert_rejected()

    def test_rejects_decoy_test_results_step(self) -> None:
        workflow = self.workflows[".github/workflows/ci.yml"]
        job = next(iter(workflow["jobs"].values()))
        job["steps"].append(
            {
                "name": "Decoy Codecov result upload",
                "uses": ACTION,
                "with": {"report_type": "test_results"},
            }
        )
        self.assert_rejected()

    def test_rejects_fifth_deprecated_step_without_report_type(self) -> None:
        workflow = self.workflows[".github/workflows/ci.yml"]
        job = next(iter(workflow["jobs"].values()))
        job["steps"].append(
            {
                "name": "Deprecated decoy upload",
                "uses": "codecov/test-results-action@old",
                "with": {"files": "decoy.xml"},
            }
        )
        self.assert_rejected()

    def test_rejects_same_name_deprecated_duplicate(self) -> None:
        workflow = self.workflows[".github/workflows/ci.yml"]
        job = next(iter(workflow["jobs"].values()))
        job["steps"].append(
            {
                "name": "Upload PR-fast test results to Codecov",
                "uses": "codecov/test-results-action@old",
                "with": {"files": "target/test-results/pr-fast-junit.xml"},
            }
        )
        self.assert_rejected()


if __name__ == "__main__":
    unittest.main()
