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
(github.event_name != 'pull_request' || github.event.pull_request.draft != true) &&
needs.route-rust-small.result == 'success' &&
(
  (needs.route-rust-small.outputs.target == 'cx53' &&
    (needs.rust-small-cx53.result == 'success' ||
     needs.rust-small-fallback.result == 'success')) ||
  (needs.route-rust-small.outputs.target == 'cx43' &&
    (needs.rust-small-cx43.result == 'success' ||
     needs.rust-small-fallback.result == 'success')) ||
  (needs.route-rust-small.outputs.target == 'github' &&
   needs.rust-small-github.result == 'success')
)
"""


def normalize_expression(value: str) -> str:
    return re.sub(r"\s+", "", value)


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


def should_probe(
    *,
    event_name: str,
    draft: bool,
    route_result: str,
    target: str,
    cx53: str = "skipped",
    cx43: str = "skipped",
    github: str = "skipped",
    fallback: str = "skipped",
) -> bool:
    event_applies = event_name != "pull_request" or not draft
    if not event_applies or route_result != "success":
        return False
    if target == "cx53":
        return cx53 == "success" or fallback == "success"
    if target == "cx43":
        return cx43 == "success" or fallback == "success"
    if target == "github":
        return github == "success"
    return False


def validate_probe_gate(workflow: str) -> None:
    expression = probe_expression(workflow)
    if normalize_expression(expression) != normalize_expression(EXPECTED_EXPRESSION):
        raise AssertionError(
            "main-red probe applicability must require successful selected proof or admitted fallback"
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
        positive = (
            dict(event_name="pull_request", draft=False, route_result="success", target="cx53", cx53="success"),
            dict(event_name="merge_group", draft=False, route_result="success", target="cx43", cx43="success"),
            dict(event_name="workflow_dispatch", draft=False, route_result="success", target="github", github="success"),
            dict(event_name="pull_request", draft=False, route_result="success", target="cx53", cx53="failure", fallback="success"),
            dict(event_name="pull_request", draft=False, route_result="success", target="cx43", cx43="failure", fallback="success"),
        )
        negative = (
            dict(event_name="pull_request", draft=True, route_result="skipped", target="github", github="skipped"),
            dict(event_name="pull_request", draft=False, route_result="failure", target="github", github="success"),
            dict(event_name="pull_request", draft=False, route_result="success", target="github", github="cancelled"),
            dict(event_name="pull_request", draft=False, route_result="success", target="cx53", cx53="cancelled"),
            dict(event_name="pull_request", draft=False, route_result="success", target="cx53", cx53="failure"),
            dict(event_name="pull_request", draft=False, route_result="success", target="cx43", cx43="cancelled"),
            dict(event_name="pull_request", draft=False, route_result="success", target="none"),
        )
        for case in positive:
            with self.subTest(case=case):
                self.assertTrue(should_probe(**case))
        for case in negative:
            with self.subTest(case=case):
                self.assertFalse(should_probe(**case))

    def test_gate_rejects_realistic_regressions(self) -> None:
        expression = probe_expression(self.workflow)
        mutations = {
            "draft-only gate": "          github.event_name != 'pull_request' || github.event.pull_request.draft != true\n",
            "router failure admitted": expression.replace(
                "needs.route-rust-small.result == 'success'",
                "needs.route-rust-small.result != 'cancelled'",
            ),
            "cancelled github admitted": expression.replace(
                "needs.rust-small-github.result == 'success'",
                "needs.rust-small-github.result == 'cancelled'",
            ),
            "cx53 fallback dropped": expression.replace(
                "needs.route-rust-small.outputs.target == 'cx53' && (needs.rust-small-cx53.result == 'success' || needs.rust-small-fallback.result == 'success')",
                "needs.route-rust-small.outputs.target == 'cx53' && needs.rust-small-cx53.result == 'success'",
            ),
            "github route dropped": expression.replace(
                " ||\n            (needs.route-rust-small.outputs.target == 'github' && needs.rust-small-github.result == 'success')",
                "",
            ),
        }
        probe_block, probe_start = step_block(self.workflow, PROBE_STEP)
        start = probe_start
        shell = probe_start + probe_block.index("        shell: bash\n")
        for name, mutated_expression in mutations.items():
            with self.subTest(name=name):
                if mutated_expression == expression:
                    self.fail(
                        f"mutation {name!r} did not apply; update the mutation anchor"
                    )
                broken = (
                    self.workflow[:start]
                    + "        if: >-\n"
                    + mutated_expression
                    + self.workflow[shell:]
                )
                with self.assertRaises(AssertionError):
                    validate_probe_gate(broken)


if __name__ == "__main__":
    unittest.main()
