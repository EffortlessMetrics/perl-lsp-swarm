#!/usr/bin/env python3
"""Structural contract for the four advisory Codecov test-result uploads."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
ACTION = "codecov/codecov-action@0fb7174895f61a3b6b78fc075e0cd60383518dac"
TOKEN = "${{ secrets.CODECOV_TOKEN }}"
CONTRACT_WORKFLOW = ROOT / ".github/workflows/workflow-contracts-advisory.yml"

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


def scalar(value: str) -> str:
    value = value.split(" #", 1)[0].strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
        return value[1:-1]
    return value


def parse_steps(path: Path) -> list[dict]:
    """Parse step-level scalar structure without a third-party YAML dependency."""
    lines = path.read_text(encoding="utf-8").splitlines()
    found: list[dict] = []
    steps_indent: int | None = None
    step: dict | None = None
    step_indent = -1
    section: str | None = None

    for line in lines:
        stripped = line.lstrip()
        indent = len(line) - len(stripped)
        if not stripped or stripped.startswith("#"):
            continue
        if steps_indent is None:
            if stripped == "steps:":
                steps_indent = indent
            continue
        if indent <= steps_indent:
            steps_indent = None
            step = None
            section = None
            if stripped == "steps:":
                steps_indent = indent
            continue
        if indent == steps_indent + 2 and stripped.startswith("- ") and ":" in stripped:
            key, value = stripped.removeprefix("- ").split(":", 1)
            step = {key: scalar(value)}
            found.append(step)
            step_indent = indent
            section = None
            continue
        if step is None or indent <= step_indent:
            continue
        if indent == step_indent + 2 and ":" in stripped:
            key, value = stripped.split(":", 1)
            if key in {"with", "env"} and not value.strip():
                step[key] = {}
                section = key
            else:
                step[key] = scalar(value)
                section = None
            continue
        if section and indent == step_indent + 4 and ":" in stripped:
            key, value = stripped.split(":", 1)
            step[section][key] = scalar(value)
    return found


def load_workflows() -> dict[str, dict]:
    return {path: {"steps": parse_steps(ROOT / path)} for path in {entry["file"] for entry in EXPECTED.values()}}


def raw_steps(workflow: dict) -> list[dict]:
    return workflow["steps"]


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

    def test_advisory_workflow_invokes_this_contract(self) -> None:
        text = CONTRACT_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn('      - "scripts/ci/test_codecov_test_results_workflows.py"', text)
        self.assertIn("python3 -m unittest scripts.ci.test_codecov_test_results_workflows", text)

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
        workflow["steps"].append(
            {
                "name": "Decoy Codecov result upload",
                "uses": ACTION,
                "with": {"report_type": "test_results"},
            }
        )
        self.assert_rejected()

    def test_rejects_fifth_deprecated_step_without_report_type(self) -> None:
        workflow = self.workflows[".github/workflows/ci.yml"]
        workflow["steps"].append(
            {
                "name": "Deprecated decoy upload",
                "uses": "codecov/test-results-action@old",
                "with": {"files": "decoy.xml"},
            }
        )
        self.assert_rejected()

    def test_rejects_same_name_deprecated_duplicate(self) -> None:
        workflow = self.workflows[".github/workflows/ci.yml"]
        workflow["steps"].append(
            {
                "name": "Upload PR-fast test results to Codecov",
                "uses": "codecov/test-results-action@old",
                "with": {"files": "target/test-results/pr-fast-junit.xml"},
            }
        )
        self.assert_rejected()

    def test_rejects_unnamed_deprecated_action_step(self) -> None:
        source = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        marker = "    steps:\n"
        mutant = source.replace(
            marker,
            marker + "      - uses: codecov/test-results-action@old\n",
            1,
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ci.yml"
            path.write_text(mutant, encoding="utf-8")
            self.workflows[".github/workflows/ci.yml"]["steps"] = parse_steps(path)
            self.assert_rejected()


if __name__ == "__main__":
    unittest.main()
