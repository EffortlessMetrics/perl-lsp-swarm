#!/usr/bin/env python3
"""Focused tests for scripts/ci/validate_gate_lane_mapping.py."""

from __future__ import annotations

import io
import json
import re
import shlex
import subprocess
import sys
import tempfile
import tomllib
import unittest

import yaml
from contextlib import redirect_stdout
from pathlib import Path


_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))

from validate_gate_lane_mapping import (  # noqa: E402
    GATE_TO_LANE_MAP,
    SCHEMA_VERSION,
    main,
    read_yaml_gate_specs,
)


def runs_full_pr_fast(script: str) -> bool:
    for line in script.replace("\\\n", " ").splitlines():
        if not re.match(r'^\s*(?:cargo\s+xtask|"?[^\s"]*/xtask"?|xtask)\s+gates\s', line):
            continue
        args = shlex.split(line, comments=True)
        if "--gate" in args or any(arg.startswith("--gate=") for arg in args):
            continue
        if "--tier=pr-fast" in args or ("--tier", "pr-fast") in zip(args, args[1:]):
            return True
    return False


class ValidateGateLaneMappingTests(unittest.TestCase):
    def test_pr_fast_workflow_executors_have_exact_tier_coverage(self) -> None:
        root = _HERE.parents[1]
        workflow = ".github/workflows/ci.yml"
        jobs = yaml.safe_load((root / workflow).read_text())["jobs"]
        whitelist = tomllib.loads((root / "policy/ci-lane-whitelist.toml").read_text())
        specs = read_yaml_gate_specs(root / ".ci/gate-policy.yaml")
        expected = {name for name, spec in specs.items() if spec["tier"] == "pr_fast"}
        executors = set()
        for job, definition in jobs.items():
            if not any(runs_full_pr_fast(step.get("run", "")) for step in definition.get("steps", [])):
                continue
            executors.add(job)
            lanes = [lane["id"] for lane in whitelist["lane"]
                     if lane.get("workflow") == workflow and lane.get("job") == job]
            self.assertEqual(1, len(lanes), job)
            actual = {name for name, entry in GATE_TO_LANE_MAP.items() if lanes[0] in entry["lanes"]}
            self.assertEqual(expected, actual, job)
        self.assertTrue({"pr-smoke", "pr-fast-unblocked"} <= executors)

    def test_full_tier_and_named_invocations(self) -> None:
        self.assertTrue(runs_full_pr_fast("cargo xtask gates --receipt --tier pr-fast"))
        self.assertFalse(runs_full_pr_fast("xtask gates --tier pr-fast --gate=unit_routed_full_build"))
        self.assertFalse(runs_full_pr_fast('echo "cargo xtask gates --tier pr-fast"'))

    def test_unrouted_advisory_gate_cannot_become_required_silently(self) -> None:
        for required, expected_status in (("false", 0), ("true", 1)):
            status, _, report = self.run_validator(
                f"gates:\n  - name: policy_checks\n    tier: merge_gate\n    required: {required}\n",
                "[lane.merge_gate_shards]\n", strict=True, json_out=True,
            )
            self.assertEqual(expected_status, status)
            self.assertEqual([], report["mapped"])
            self.assertEqual(required == "false", "policy_checks" in report["intentionally_unrouted"])

    def run_validator(
        self,
        gate_policy_text: str,
        lanes_text: str,
        *,
        strict: bool,
        json_out: bool = False,
        workflow_text: str | None = None,
        workflow_texts: dict[str, str] | None = None,
    ) -> tuple[int, str, dict[str, object] | None]:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            gate_policy = root / "gate-policy.yaml"
            lanes = root / "ci-lanes.toml"
            report = root / "reports" / "gate-lane-mapping.json"
            gate_policy.write_text(gate_policy_text, encoding="utf-8")
            lanes.write_text(lanes_text, encoding="utf-8")
            if workflow_text is None:
                names = re.findall(r"^\s*- name:\s*([a-z][a-z0-9_]*)", gate_policy_text, re.M)
                workflow_text = "jobs:\n  test:\n    gates: " + " ".join(names) + "\n"
            workflows = {"ci.yml": workflow_text}
            workflows.update(workflow_texts or {})
            workflow_paths = []
            for name, contents in workflows.items():
                workflow_path = root / name
                workflow_path.parent.mkdir(parents=True, exist_ok=True)
                workflow_path.write_text(contents, encoding="utf-8")
                workflow_paths.append(workflow_path)

            old_argv = sys.argv
            try:
                sys.argv = [
                    "validate_gate_lane_mapping.py",
                    "--gate-policy",
                    str(gate_policy),
                    "--lanes",
                    str(lanes),
                ]
                for workflow_path in sorted(workflow_paths):
                    sys.argv.extend(["--workflow", str(workflow_path)])
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

    def test_json_out_report_includes_schema_version(self) -> None:
        status, _, report = self.run_validator(
            """
gates:
  - name: fmt
""",
            """
[lane.pr_smoke]
[lane.pr_fast_unblocked]
""",
            strict=False,
            json_out=True,
        )

        self.assertEqual(0, status)
        assert report is not None
        self.assertEqual("gate_lane_mapping.v1", SCHEMA_VERSION)
        self.assertEqual(SCHEMA_VERSION, report["schema_version"])
        self.assertTrue(json.dumps(report).startswith('{"schema_version":'))

    def test_strict_passes_when_every_gate_maps_to_existing_lane(self) -> None:
        status, output, report = self.run_validator(
            """
gates:
  - name: fmt
  - name: docs_build
""",
            """
[lane.pr_smoke]
[lane.pr_fast_unblocked]
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

    def test_strict_rejects_a_mapping_left_after_gate_removal(self) -> None:
        repo = _HERE.parents[1]
        gate_policy = Path(".ci/gate-policy.yaml")
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for path in (
                gate_policy,
                Path("policy/ci-lanes.toml"),
                Path("docs/ci/gate-policy-economics.md"),
            ):
                target = root / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes((repo / path).read_bytes())

            command = [sys.executable, str(_HERE / "validate_gate_lane_mapping.py"), "--strict"]
            valid = subprocess.run(command, cwd=root, capture_output=True, text=True)
            self.assertEqual(0, valid.returncode, valid.stdout + valid.stderr)

            policy = root / gate_policy
            without_gate, removed = re.subn(
                r"(?ms)^  - name: whitespace_check\n.*?(?=^  - name: )",
                "",
                policy.read_text(encoding="utf-8"),
            )
            self.assertEqual(1, removed)
            policy.write_text(without_gate, encoding="utf-8")

            stale = subprocess.run(command, cwd=root, capture_output=True, text=True)
            self.assertEqual(1, stale.returncode, stale.stdout + stale.stderr)
            self.assertIn("Mappings to gates absent from .ci/gate-policy.yaml: 1", stale.stdout)
            self.assertIn("  - whitespace_check\n", stale.stdout)
            self.assertIn("Docs lane-enumeration drift: 0", stale.stdout)

    def test_must_context_check_maps_to_merge_gate_shards(self) -> None:
        status, output, report = self.run_validator(
            """
gates:
  - name: must_context_check
    tier: merge_gate
    required: false
""",
            """
[lane.merge_gate_shards]
""",
            strict=True,
            json_out=True,
        )

        self.assertEqual(0, status)
        self.assertIn("Unmapped gates: 0", output)
        assert report is not None
        self.assertEqual([], report["unmapped_gates"])
        self.assertEqual(
            [{"gate": "must_context_check", "lanes": ["merge_gate_shards"]}],
            report["mapped"],
        )
        self.assertEqual(
            ["merge_gate_shards"], GATE_TO_LANE_MAP["must_context_check"]["lanes"]
        )

    def test_docs_agents_contract_workflows_maps_to_merge_gate_shards(self) -> None:
        status, output, report = self.run_validator(
            """
gates:
  - name: docs_agents_contract_workflows
    tier: merge_gate
    required: true
""",
            """
[lane.merge_gate_shards]
""",
            strict=True,
            json_out=True,
        )

        self.assertEqual(0, status)
        self.assertIn("Unmapped gates: 0", output)
        assert report is not None
        self.assertEqual([], report["unmapped_gates"])
        self.assertEqual(
            [{"gate": "docs_agents_contract_workflows", "lanes": ["merge_gate_shards"]}],
            report["mapped"],
        )
        self.assertEqual(
            ["merge_gate_shards"],
            GATE_TO_LANE_MAP["docs_agents_contract_workflows"]["lanes"],
        )

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
[lane.pr_fast_unblocked]
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

    def test_strict_accepts_required_gate_reachable_in_second_workflow(self) -> None:
        status, output, report = self.run_validator(
            """
gates:
  - name: docs_build
    tier: merge_gate
    required: true
""",
            """
[lane.docs_gate]
""",
            strict=True,
            json_out=True,
            workflow_text="""
jobs:
  test:
    run: echo no release gates here
""",
            workflow_texts={
                "release.yml": """
jobs:
  release:
    gates: docs_build
"""
            },
        )

        self.assertEqual(0, status)
        self.assertIn("Required unreachable gates: 0", output)
        assert report is not None
        self.assertEqual([], report["required_unreachable_gates"])

    def test_strict_finds_tier_gate_inside_block_scalar_run_step(self) -> None:
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
[lane.pr_fast_unblocked]
[lane.docs_gate]
""",
            strict=True,
            json_out=True,
            workflow_text="""
jobs:
  test:
    steps:
      - name: Run PR-fast
        run: |
          set +e
          cargo xtask gates --tier pr-fast --base origin/main
""",
        )

        self.assertEqual(1, status)
        self.assertIn("Required unreachable gates: 1", output)
        self.assertIn("  - docs_build", output)
        self.assertNotIn("  - fmt", output)
        assert report is not None
        self.assertEqual(["docs_build"], report["required_unreachable_gates"])

    def test_strict_ignores_gate_like_strings_in_workflow_metadata(self) -> None:
        status, output, report = self.run_validator(
            """
gates:
  - name: docs_build
    tier: merge_gate
    required: true
  - name: common_corpus_clean
    tier: merge_gate
    required: true
""",
            """
[lane.docs_gate]
[lane.merge_gate_shards]
""",
            strict=True,
            json_out=True,
            workflow_text="""
name: "gates --tier merge_gate"
jobs:
  test:
    env:
      NOTE: "gates --gate common_corpus_clean"
    run: echo hello
""",
        )

        self.assertEqual(1, status)
        self.assertIn("Required unreachable gates: 2", output)
        self.assertIn("  - docs_build", output)
        self.assertIn("  - common_corpus_clean", output)
        assert report is not None
        self.assertEqual(
            ["common_corpus_clean", "docs_build"],
            report["required_unreachable_gates"],
        )

    def test_strict_fails_on_unmapped_gate_and_missing_lane_reference(self) -> None:
        status, output, report = self.run_validator(
            """
gates:
  - name: check_conflict_markers
  - name: new_quality_gate
""",
            """
[lane.pr_smoke]
[lane.pr_fast_unblocked]
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
[lane.pr_fast_unblocked]
""",
            strict=False,
        )

        self.assertEqual(0, status)
        self.assertIn("Mapped: 2", output)
        self.assertIn("Mapped to non-existent lanes: 1", output)
        self.assertIn("check_conflict_markers -> conflict_markers", output)


if __name__ == "__main__":
    unittest.main()
