#!/usr/bin/env python3
"""Falsifiers for required-check registration diagnosis (#16469).

Discriminators:

- every required context is success on the exact head, and GitHub merge
  evaluation is not refusing → COMPLETE_GREEN
- a required context is absent from every SHA → REQUIRED_ABSENT (#14612)
- a required context is reported only on another SHA → REQUIRED_OTHER_SHA
- every required context is success on the exact head, but GitHub still
  reports expected checks → EXPECTED_STALE (#16469)
- merge BLOCKED without an expected-checks signal is NOT_PROVEN, not a
  GitHub-defect inference
- skipped required jobs on the current head are not success
- malformed input is NOT_PROVEN, never green
"""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path

_HERE = Path(__file__).resolve().parent
ROOT = _HERE.parents[1]
SCRIPT = _HERE / "detect_required_check_registration.py"
POLICY = ROOT / ".ci" / "policies" / "required-checks.toml"
CONTRACT_SCRIPT = _HERE / "validate_gate_enforcement_contract.py"

SPEC = importlib.util.spec_from_file_location("detect_required_check_registration", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
detector = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = detector
SPEC.loader.exec_module(detector)

CONTRACT_SPEC = importlib.util.spec_from_file_location("gate_enforcement_contract", CONTRACT_SCRIPT)
assert CONTRACT_SPEC is not None and CONTRACT_SPEC.loader is not None
contract = importlib.util.module_from_spec(CONTRACT_SPEC)
sys.modules[CONTRACT_SPEC.name] = contract
CONTRACT_SPEC.loader.exec_module(contract)

HEAD = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
OTHER = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
REQUIRED = (
    "Perl LSP Rust Small Result",
    "ripr+ New Gap Gate",
    "Compile All Targets (bit-rot guard)",
    "Conflict marker check",
    "validate-title",
)
CONDITIONAL_REQUIRED_JOBS = {
    ("Compile All Targets (bit-rot guard)", "check-all-targets"),
    ("Conflict marker check", "conflict-markers"),
}


def payload(**overrides):
    data = {
        "head_sha": HEAD,
        "required_contexts": list(REQUIRED),
        "check_runs": [
            {"name": name, "head_sha": HEAD, "status": "completed", "conclusion": "success"}
            for name in REQUIRED
        ],
        "merge_evaluation": {
            "mergeable": "MERGEABLE",
            "merge_state_status": "CLEAN",
            "blocked_by_expected_checks": False,
        },
    }
    data.update(overrides)
    return data


def classify(data):
    return detector.classify(data)


def run_cli(data, extra_args=None):
    args = [sys.executable, str(SCRIPT)]
    if extra_args:
        args.extend(extra_args)
    return subprocess.run(
        args,
        input=json.dumps(data),
        text=True,
        capture_output=True,
        check=False,
    )


class ClassifierTests(unittest.TestCase):
    def test_all_required_success_on_head_with_clean_merge_is_complete_green(self) -> None:
        result = classify(payload())
        self.assertEqual("COMPLETE_GREEN", result["verdict"])
        self.assertEqual(list(REQUIRED), result["required_contexts"])
        self.assertFalse(result["rows"])

    def test_required_context_absent_is_14612_shape_not_16469(self) -> None:
        data = payload()
        data["check_runs"] = [row for row in data["check_runs"] if row["name"] != "ripr+ New Gap Gate"]
        result = classify(data)
        self.assertEqual("REQUIRED_ABSENT", result["verdict"])
        self.assertEqual(["ripr+ New Gap Gate"], result["absent"])
        self.assertIn("#14612", result["remedy"])
        self.assertNotIn("empty commit", result["remedy"].lower())
        self.assertNotEqual("EXPECTED_STALE", result["verdict"])

    def test_required_context_only_on_other_sha_is_stale_subject(self) -> None:
        data = payload()
        for row in data["check_runs"]:
            if row["name"] == "validate-title":
                row["head_sha"] = OTHER
        result = classify(data)
        self.assertEqual("REQUIRED_OTHER_SHA", result["verdict"])
        self.assertEqual(["validate-title"], result["other_sha"])
        self.assertIn(OTHER[:12], result["remedy"])
        self.assertIn(HEAD[:12], result["remedy"])

    def test_green_head_with_explicit_expected_checks_is_16469(self) -> None:
        data = payload(
            merge_evaluation={
                "mergeable": "MERGEABLE",
                "merge_state_status": "BLOCKED",
                "blocked_by_expected_checks": True,
                "blocked_reason": "required status checks are expected",
            }
        )
        result = classify(data)
        self.assertEqual("EXPECTED_STALE", result["verdict"])
        self.assertIn("#16469", result["remedy"])
        self.assertIn("#14612", result["remedy"])
        self.assertIn("not #14612", result["remedy"].lower())
        self.assertIn("rerequest", result["remedy"].lower())
        self.assertIn("workflow_dispatch", result["remedy"].lower())
        self.assertIn("empty commit", result["remedy"].lower())
        self.assertIn("do not push an empty commit as the default", result["remedy"].lower())

    def test_blocked_merge_without_expected_signal_is_not_inferred_github_defect(self) -> None:
        data = payload(
            merge_evaluation={
                "mergeable": "MERGEABLE",
                "merge_state_status": "BLOCKED",
            }
        )
        result = classify(data)
        self.assertEqual("NOT_PROVEN", result["verdict"])
        self.assertIn("without an expected-checks signal", result["remedy"])
        self.assertNotEqual("EXPECTED_STALE", result["verdict"])
        self.assertNotEqual("COMPLETE_GREEN", result["verdict"])

    def test_conflicting_tree_is_not_expected_stale(self) -> None:
        data = payload(
            merge_evaluation={
                "mergeable": "CONFLICTING",
                "merge_state_status": "DIRTY",
                "blocked_by_expected_checks": True,
                "blocked_reason": "required status checks are expected",
            }
        )
        result = classify(data)
        self.assertEqual("NOT_PROVEN", result["verdict"])
        self.assertIn("conflict", result["remedy"].lower())

    def test_skipped_required_job_is_not_success(self) -> None:
        data = payload()
        for row in data["check_runs"]:
            if row["name"] == "Compile All Targets (bit-rot guard)":
                row["conclusion"] = "skipped"
        result = classify(data)
        self.assertEqual("REQUIRED_SKIPPED", result["verdict"])
        self.assertEqual(["Compile All Targets (bit-rot guard)"], result["skipped"])
        self.assertIn("always-or-scoped-noop", result["remedy"])
        self.assertNotEqual("COMPLETE_GREEN", result["verdict"])

    def test_red_required_context_blocks(self) -> None:
        data = payload()
        for row in data["check_runs"]:
            if row["name"] == "Conflict marker check":
                row["conclusion"] = "failure"
        result = classify(data)
        self.assertEqual("REQUIRED_RED", result["verdict"])
        self.assertEqual(["Conflict marker check"], result["red"])

    def test_pending_required_context_blocks(self) -> None:
        data = payload()
        for row in data["check_runs"]:
            if row["name"] == "Perl LSP Rust Small Result":
                row["status"] = "in_progress"
                row["conclusion"] = None
        result = classify(data)
        self.assertEqual("REQUIRED_PENDING", result["verdict"])
        self.assertEqual(["Perl LSP Rust Small Result"], result["pending"])

    def test_head_green_without_merge_evaluation_is_not_proven(self) -> None:
        data = payload()
        del data["merge_evaluation"]
        result = classify(data)
        self.assertEqual("NOT_PROVEN", result["verdict"])
        self.assertEqual("HEAD_GREEN", result["head_status"])
        self.assertIn("merge evaluation is missing", result["remedy"])

    def test_missing_head_sha_is_not_proven_never_green(self) -> None:
        data = payload()
        del data["head_sha"]
        result = classify(data)
        self.assertEqual("NOT_PROVEN", result["verdict"])
        self.assertNotEqual("COMPLETE_GREEN", result["verdict"])

    def test_empty_required_list_is_not_proven(self) -> None:
        result = classify(payload(required_contexts=[], check_runs=[]))
        self.assertEqual("NOT_PROVEN", result["verdict"])
        self.assertIn("no required contexts", result["remedy"])

    def test_cli_complete_green_exits_zero_and_prints_json(self) -> None:
        proc = run_cli(payload())
        self.assertEqual(0, proc.returncode, proc.stderr)
        body = json.loads(proc.stdout)
        self.assertEqual("COMPLETE_GREEN", body["verdict"])

    def test_cli_expected_stale_exits_nonzero(self) -> None:
        proc = run_cli(
            payload(
                merge_evaluation={
                    "mergeable": "MERGEABLE",
                    "merge_state_status": "BLOCKED",
                    "blocked_by_expected_checks": True,
                    "blocked_reason": "checks are expected",
                }
            )
        )
        self.assertEqual(1, proc.returncode)
        body = json.loads(proc.stdout)
        self.assertEqual("EXPECTED_STALE", body["verdict"])
        self.assertIn("EXPECTED_STALE", proc.stderr)

    def test_policy_loader_matches_checked_in_required_names(self) -> None:
        names = detector.required_names_from_policy(POLICY.read_text(encoding="utf-8"))
        self.assertEqual(tuple(sorted(REQUIRED)), names)


def _live_workflows() -> dict:
    """Reuse the contract-test catalog cache when both modules share a process."""
    for name, module in sys.modules.items():
        live = getattr(module, "_live_catalog", None)
        if callable(live) and name.endswith("test_validate_gate_enforcement_contract"):
            workflows, _producers = live()
            return workflows
    return contract.read_workflow_catalog(ROOT)


class LiveWorkflowStructureTests(unittest.TestCase):
    """Every required name must have a static always-reporting or known-conditional job."""

    @classmethod
    def setUpClass(cls) -> None:
        policy = tomllib.loads(POLICY.read_text(encoding="utf-8"))
        cls.required_rows = [
            row
            for row in policy.get("checks", [])
            if row.get("required") is True
        ]
        cls.workflows = _live_workflows()

    def test_every_required_name_has_a_static_job_with_the_same_name(self) -> None:
        missing = []
        for row in self.required_rows:
            workflow = self.workflows.get(row["workflow"])
            job = None if workflow is None else workflow.jobs.get(row["job"])
            if job is None or job.name != row["name"] or not job.name_static:
                missing.append(row["name"])
        self.assertEqual([], missing)

    def test_always_or_scoped_noop_required_jobs_are_unconditionally_reachable(self) -> None:
        mismatched = []
        for row in self.required_rows:
            if row.get("applicability") != "always-or-scoped-noop":
                continue
            job = self.workflows[row["workflow"]].jobs[row["job"]]
            if job.condition_class != "always":
                mismatched.append((row["name"], job.condition_class))
        self.assertEqual([], mismatched)

    def test_conditional_required_jobs_are_the_known_draft_or_superseded_pair(self) -> None:
        observed = {
            (row["name"], row["job"])
            for row in self.required_rows
            if row.get("applicability") == "conditional"
        }
        self.assertEqual(CONDITIONAL_REQUIRED_JOBS, observed)
        for name, job_id in sorted(observed):
            row = next(item for item in self.required_rows if item["name"] == name)
            job = self.workflows[row["workflow"]].jobs[job_id]
            self.assertEqual("conditional", job.condition_class, name)

    def test_required_events_are_not_path_filtered(self) -> None:
        filtered = []
        for row in self.required_rows:
            workflow = self.workflows[row["workflow"]]
            for event in row.get("events", []):
                if event in workflow.path_filtered_events:
                    filtered.append((row["name"], event))
        self.assertEqual([], filtered)


class RecoveryContractTests(unittest.TestCase):
    def test_expected_stale_remedy_refuses_empty_commit_default(self) -> None:
        remedy = detector.REMEDY["EXPECTED_STALE"]
        self.assertIn("Do not push an empty commit as the default", remedy)
        self.assertIn("POST /repos/{owner}/{repo}/check-suites/{id}/rerequest", remedy)
        self.assertIn("workflow_dispatch", remedy)

    def test_absent_remedy_does_not_treat_silence_as_success(self) -> None:
        remedy = detector.REMEDY["REQUIRED_ABSENT"]
        self.assertIn("never treat absence as success", remedy.lower())
        self.assertIn("#14612", remedy)


class FixtureFileTests(unittest.TestCase):
    def test_cli_reads_fixture_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "fixture.json"
            path.write_text(json.dumps(payload()), encoding="utf-8")
            proc = subprocess.run(
                [sys.executable, str(SCRIPT), "--fixture", str(path)],
                text=True,
                capture_output=True,
                check=False,
            )
        self.assertEqual(0, proc.returncode, proc.stderr)
        self.assertEqual("COMPLETE_GREEN", json.loads(proc.stdout)["verdict"])


if __name__ == "__main__":
    unittest.main()
