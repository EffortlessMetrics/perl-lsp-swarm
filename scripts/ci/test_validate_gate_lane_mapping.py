#!/usr/bin/env python3
"""Focused tests for scripts/ci/validate_gate_lane_mapping.py."""

from __future__ import annotations

import io
import json
import re
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path


_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))

from validate_gate_lane_mapping import main  # noqa: E402


class ValidateGateLaneMappingTests(unittest.TestCase):
    def run_validator(
        self,
        gate_policy_text: str,
        lanes_text: str,
        *,
        strict: bool,
        json_out: bool = False,
        workflow_text: str | None = None,
    ) -> tuple[int, str, dict[str, object] | None]:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            gate_policy = root / "gate-policy.yaml"
            lanes = root / "ci-lanes.toml"
            workflow = root / "ci.yml"
            report = root / "reports" / "gate-lane-mapping.json"
            gate_policy.write_text(gate_policy_text, encoding="utf-8")
            lanes.write_text(lanes_text, encoding="utf-8")
            if workflow_text is None:
                names = re.findall(r"^\s*- name:\s*([a-z][a-z0-9_]*)", gate_policy_text, re.M)
                workflow_text = "jobs:\n  test:\n    gates: " + " ".join(names) + "\n"
            workflow.write_text(workflow_text, encoding="utf-8")

            old_argv = sys.argv
            try:
                sys.argv = [
                    "validate_gate_lane_mapping.py",
                    "--gate-policy",
                    str(gate_policy),
                    "--lanes",
                    str(lanes),
                    "--workflow",
                    str(workflow),
                ]
                if strict:
                    sys.argv.append("--strict")
                if json_out:
                    sys.argv.extend(["--json-out", str(report)])
                stdout = io.StringIO()
                with redirect_stdout(stdout):
                    status = main()
            finally:
                sys.argv = old_argv

            report_json = json.loads(report.read_text(encoding="utf-8")) if json_out else None

        return status, stdout.getvalue(), report_json

    def test_strict_passes_when_every_gate_maps_to_existing_lane(self) -> None:
        status, output, report = self.run_validator(
            """
gates:
  - name: fmt
  - name: docs_build
""",
            """
[lane.pr_smoke]
[lane.docs_gate]
""",
            strict=True,
            json_out=True,
        )

        self.assertEqual(0, status)
        self.assertIn("Gates in", output)
        self.assertIn("Mapped: 2", output)
        self.assertIn("Unmapped gates: 0", output)
        self.assertIsNotNone(report)
        assert report is not None
        self.assertEqual(2, report["gate_count"])
        self.assertEqual([], report["unmapped_gates"])
        self.assertEqual([], report["missing_lanes"])

    def test_strict_fails_when_required_gate_has_no_workflow_path(self) -> None:
        status, output, report = self.run_validator(
            """
gates:
  - name: fmt
    tier: pr_fast
    required: true
  - name: docs_build
    tier: merge_gate
    required: true
""",
            """
[lane.pr_smoke]
[lane.docs_gate]
""",
            strict=True,
            json_out=True,
            workflow_text="""
jobs:
  test:
    run: cargo xtask gates --tier pr_fast
""",
        )

        self.assertEqual(1, status)
        self.assertIn("Required unreachable gates: 1", output)
        self.assertIn("  - docs_build", output)
        assert report is not None
        self.assertEqual(["docs_build"], report["required_unreachable_gates"])

    def test_strict_fails_on_unmapped_gate_and_missing_lane_reference(self) -> None:
        status, output, report = self.run_validator(
            """
gates:
  - name: check_conflict_markers
  - name: new_quality_gate
""",
            """
[lane.pr_smoke]
""",
            strict=True,
            json_out=True,
        )

        self.assertEqual(1, status)
        self.assertIn("Unmapped gates: 1", output)
        self.assertIn("  - new_quality_gate", output)
        self.assertIn("Mapped to non-existent lanes: 1", output)
        self.assertIn(
            "  - check_conflict_markers -> conflict_markers  (lane not in ci-lanes.toml)",
            output,
        )
        self.assertIsNotNone(report)
        assert report is not None
        self.assertEqual(["new_quality_gate"], report["unmapped_gates"])
        self.assertEqual(
            [{"gate": "check_conflict_markers", "lane": "conflict_markers"}],
            report["missing_lanes"],
        )

    def test_non_strict_reports_mismatch_without_failing(self) -> None:
        status, output, _ = self.run_validator(
            """
gates:
  - name: check_conflict_markers
  - name: release_history
""",
            """
[lane.pr_smoke]
""",
            strict=False,
        )

        self.assertEqual(0, status)
        self.assertIn("Mapped: 2", output)
        self.assertIn("Mapped to non-existent lanes: 1", output)
        self.assertIn("check_conflict_markers -> conflict_markers", output)


if __name__ == "__main__":
    unittest.main()
