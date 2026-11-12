#!/usr/bin/env python3
"""Contract tests for Rust Small main-red probe applicability (#13595)."""

from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "em-ci-routed-rust.yml"
PROBE_STEP = "Probe main-red refusal"
EVALUATE_STEP = "Evaluate routed result"
STRUCTURAL_STEP = "Prove rustfmt prevention contract"
CONTRACT_TEST = "scripts/ci/test_rust_small_probe_gate.py"
EXPECTED_EXPRESSION = """
github.event_name == 'merge_group' &&
needs.rust-small-github.result == 'success'
"""


def normalize_expression(value: str) -> str:
    # GitHub expressions escape an apostrophe by doubling it. Only discard
    # layout between tokens; whitespace inside a comparison value is semantic.
    return re.sub(
        r"'(?:[^']|'')*'|\s+",
        lambda match: match.group(0) if match.group(0).startswith("'") else "",
        value,
    )


def step_block(workflow: str, step_name: str) -> tuple[str, int]:
    pattern = re.compile(
        rf"(?ms)^(?P<indent>[ \t]*)- name: {re.escape(step_name)}[ \t]*\n"
        rf"(?P<body>.*?)(?=^(?P=indent)- name: |\Z)"
    )
    match = pattern.search(workflow)
    if match is None:
        raise AssertionError(f"workflow step not found: {step_name}")
    return match.group(0), match.start()


def probe_expression(workflow: str) -> str:
    block, _ = step_block(workflow, PROBE_STEP)
    if_match = re.search(r"(?m)^(?P<indent>[ \t]+)if: >-[ \t]*$", block)
    if if_match is None:
        raise AssertionError("main-red probe must use a multiline applicability expression")
    shell_match = re.search(
        rf"(?m)^{re.escape(if_match.group('indent'))}shell:",
        block[if_match.end() :],
    )
    if shell_match is None:
        raise AssertionError("main-red probe must declare its shell after the applicability expression")
    return block[if_match.end() : if_match.end() + shell_match.start()]


def replace_probe_expression(workflow: str, expression: str) -> str:
    block, block_start = step_block(workflow, PROBE_STEP)
    if_match = re.search(r"(?m)^(?P<indent>[ \\t]+)if: >-[ \\t]*$", block)
    if if_match is None:
        raise AssertionError("main-red probe must use a multiline applicability expression")
    shell_match = re.search(
        rf"(?m)^{re.escape(if_match.group('indent'))}shell:",
        block[if_match.end() :],
    )
    if shell_match is None:
        raise AssertionError("main-red probe must declare its shell after the applicability expression")
    expression_start = block_start + if_match.end()
    expression_end = expression_start + shell_match.start()
    return workflow[:expression_start] + expression + workflow[expression_end:]


def should_probe(*, event_name: str, github: str) -> bool:
    return event_name == "merge_group" and github == "success"


def validate_probe_gate(workflow: str) -> None:
    expression = probe_expression(workflow)
    if normalize_expression(expression) != normalize_expression(EXPECTED_EXPRESSION):
        raise AssertionError(
            "main-red probe applicability must require successful hosted proof on actual G"
        )

    _, structural = step_block(workflow, STRUCTURAL_STEP)
    _, probe = step_block(workflow, PROBE_STEP)
    _, evaluate = step_block(workflow, EVALUATE_STEP)
    if not structural < probe < evaluate:
        raise AssertionError(
            "probe applicability must be evaluated after structural contracts and before final route evaluation"
        )
    if CONTRACT_TEST not in workflow[structural:probe]:
        raise AssertionError("Rust Small aggregate must execute the probe-gate contract before probing")


def load_workflow() -> str:
    return WORKFLOW_PATH.read_text(encoding="utf-8")


class RustSmallProbeGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = load_workflow()

    def test_checked_in_workflow_matches_probe_gate(self) -> None:
        validate_probe_gate(self.workflow)

    def test_truth_table_preserves_success_and_rejects_absent_proof(self) -> None:
        for event in ("merge_group", "pull_request", "pull_request_target", "push", "workflow_dispatch"):
            for result in ("success", "failure", "cancelled", "skipped", "", "unknown"):
                with self.subTest(event=event, result=result):
                    self.assertEqual(should_probe(event_name=event, github=result),
                                     event == "merge_group" and result == "success")

    def test_gate_rejects_realistic_regressions(self) -> None:
        expression = probe_expression(self.workflow)
        mutations = {
            "prequeue event admitted": expression.replace("'merge_group'", "'pull_request'"),
            "failure admitted": expression.replace("'success'", "'failure'"),
            "cancellation admitted": expression.replace("'success'", "'cancelled'"),
            "success literal drift": expression.replace("'success'", "'success '"),
            "proof not required": "          github.event_name == 'merge_group'\n",
            "group not required": "          needs.rust-small-github.result == 'success'\n",
        }
        for name, changed in mutations.items():
            with self.subTest(name=name):
                self.assertNotEqual(changed, expression)
                with self.assertRaises(AssertionError):
                    validate_probe_gate(replace_probe_expression(self.workflow, changed))

    def test_normalization_preserves_quoted_whitespace(self) -> None:
        self.assertEqual(
            normalize_expression("  value == 'it''s a label ' \n && other == ' ready' "),
            "value=='it''s a label '&&other==' ready'",
        )


if __name__ == "__main__":
    unittest.main()
