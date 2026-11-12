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
- a skipped required job with the bound app on the classified SHA is
  GitHub-accepted registration, recorded as product_not_run, not REQUIRED_SKIPPED
- workflow-filtered absence (no check-run) remains REQUIRED_ABSENT
- a same-name run from a different app does not satisfy the required identity
- an unfinished same-head rerun vetoes an older completed green
- empty merge evaluation is NOT_PROVEN, never green
- GitHub `neutral` is success
- COMPLETE_GREEN is a subject observation, not a protected merge verdict when
  merge_commit_sha differs from the classified SHA
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
MERGE = "cccccccccccccccccccccccccccccccccccccccc"
GITHUB_ACTIONS_APP = 15368
OTHER_APP = 999
REQUIRED = (
    "Perl LSP Rust Small Result",
    "ripr+ New Gap Gate",
    "Compile All Targets (bit-rot guard)",
    "Conflict marker check",
    "validate-title",
    "Gate Enforcement Contract",
)
CONDITIONAL_REQUIRED_JOBS = {
    ("ripr+ New Gap Gate", "ripr"),
    ("Conflict marker check", "conflict-markers"),
}


def required_bindings():
    return [{"name": name, "app_id": GITHUB_ACTIONS_APP} for name in REQUIRED]


def payload(**overrides):
    data = {
        "head_sha": HEAD,
        "required_contexts": required_bindings(),
        "check_runs": [
            {
                "name": name,
                "head_sha": HEAD,
                "status": "completed",
                "conclusion": "success",
                "app": {"id": GITHUB_ACTIONS_APP},
            }
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
        self.assertIn("does not satisfy", result["remedy"].lower())
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
        self.assertIn("does not count", result["remedy"].lower())
        self.assertIn("#17900", result["remedy"])
        self.assertIn("#17903", result["remedy"])
        self.assertIn("do not push an empty commit", result["remedy"].lower())

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

    def test_skipped_required_job_is_github_accepted_registration(self) -> None:
        data = payload()
        for row in data["check_runs"]:
            if row["name"] == "Compile All Targets (bit-rot guard)":
                row["conclusion"] = "skipped"
        result = classify(data)
        self.assertEqual("COMPLETE_GREEN", result["verdict"])
        self.assertEqual(["Compile All Targets (bit-rot guard)"], result["product_not_run"])
        self.assertNotEqual("REQUIRED_SKIPPED", result["verdict"])

    def test_workflow_silence_is_absent_not_skipped(self) -> None:
        data = payload()
        data["check_runs"] = [
            row for row in data["check_runs"]
            if row["name"] != "Compile All Targets (bit-rot guard)"
        ]
        result = classify(data)
        self.assertEqual("REQUIRED_ABSENT", result["verdict"])
        self.assertEqual(["Compile All Targets (bit-rot guard)"], result["absent"])
        self.assertIn("does not satisfy", result["remedy"].lower())

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

    def test_policy_loader_carries_ruleset_integration_id(self) -> None:
        contexts = detector.required_contexts_from_policy(POLICY.read_text(encoding="utf-8"))
        self.assertEqual(
            [{"name": name, "app_id": GITHUB_ACTIONS_APP} for name in sorted(REQUIRED)],
            contexts,
        )

    def test_cli_policy_supplies_app_bound_contexts(self) -> None:
        data = payload()
        del data["required_contexts"]
        proc = run_cli(data, extra_args=["--policy", str(POLICY)])
        self.assertEqual(0, proc.returncode, proc.stderr)
        self.assertEqual("COMPLETE_GREEN", json.loads(proc.stdout)["verdict"])

    def test_string_only_required_contexts_are_not_proven(self) -> None:
        result = classify(payload(required_contexts=list(REQUIRED)))
        self.assertEqual("NOT_PROVEN", result["verdict"])
        self.assertIn("app_id", result["remedy"])
        self.assertNotEqual("COMPLETE_GREEN", result["verdict"])

    def test_same_name_wrong_app_does_not_satisfy_required_context(self) -> None:
        data = payload()
        for row in data["check_runs"]:
            if row["name"] == "ripr+ New Gap Gate":
                row["app"] = {"id": OTHER_APP}
        result = classify(data)
        self.assertEqual("REQUIRED_ABSENT", result["verdict"])
        self.assertEqual(["ripr+ New Gap Gate"], result["absent"])
        self.assertNotEqual("COMPLETE_GREEN", result["verdict"])
        self.assertNotEqual("EXPECTED_STALE", result["verdict"])

    def test_missing_app_on_matching_run_is_not_proven(self) -> None:
        data = payload()
        for row in data["check_runs"]:
            if row["name"] == "ripr+ New Gap Gate":
                del row["app"]
        result = classify(data)
        self.assertEqual("NOT_PROVEN", result["verdict"])
        self.assertIn("app", result["remedy"].lower())
        self.assertNotEqual("COMPLETE_GREEN", result["verdict"])

    def test_green_plus_in_progress_same_head_rerun_is_pending(self) -> None:
        data = payload()
        data["check_runs"].append(
            {
                "name": "Perl LSP Rust Small Result",
                "head_sha": HEAD,
                "status": "in_progress",
                "conclusion": None,
                "app": {"id": GITHUB_ACTIONS_APP},
            }
        )
        result = classify(data)
        self.assertEqual("REQUIRED_PENDING", result["verdict"])
        self.assertEqual(["Perl LSP Rust Small Result"], result["pending"])
        self.assertNotEqual("COMPLETE_GREEN", result["verdict"])
        self.assertNotEqual("EXPECTED_STALE", result["verdict"])

    def test_empty_merge_evaluation_is_not_proven(self) -> None:
        result = classify(payload(merge_evaluation={}))
        self.assertEqual("NOT_PROVEN", result["verdict"])
        self.assertNotEqual("COMPLETE_GREEN", result["verdict"])

    def test_partial_merge_evaluation_is_not_proven(self) -> None:
        result = classify(payload(merge_evaluation={"mergeable": "MERGEABLE"}))
        self.assertEqual("NOT_PROVEN", result["verdict"])
        self.assertNotEqual("COMPLETE_GREEN", result["verdict"])

    def test_neutral_on_head_is_success(self) -> None:
        data = payload()
        for row in data["check_runs"]:
            if row["name"] == "validate-title":
                row["conclusion"] = "neutral"
        result = classify(data)
        self.assertEqual("COMPLETE_GREEN", result["verdict"])
        self.assertFalse(result["product_not_run"])

    def test_merge_commit_sha_different_from_head_is_not_protected_merge_verdict(self) -> None:
        data = payload(
            merge_evaluation={
                "mergeable": "MERGEABLE",
                "merge_state_status": "CLEAN",
                "blocked_by_expected_checks": False,
                "merge_commit_sha": MERGE,
            }
        )
        result = classify(data)
        self.assertEqual("NOT_PROVEN", result["verdict"])
        self.assertIn("merge commit", result["remedy"].lower())
        self.assertNotEqual("COMPLETE_GREEN", result["verdict"])
        self.assertNotEqual("EXPECTED_STALE", result["verdict"])

    def test_expected_checks_with_unobserved_merge_commit_is_not_stale_github_defect(self) -> None:
        data = payload(
            merge_evaluation={
                "mergeable": "MERGEABLE",
                "merge_state_status": "BLOCKED",
                "blocked_by_expected_checks": True,
                "blocked_reason": "required status checks are expected",
                "merge_commit_sha": MERGE,
            }
        )
        result = classify(data)
        self.assertEqual("NOT_PROVEN", result["verdict"])
        self.assertNotEqual("EXPECTED_STALE", result["verdict"])

    def test_evaluated_sha_on_merge_commit_is_complete_green(self) -> None:
        data = payload(
            evaluated_sha=MERGE,
            check_runs=[
                {
                    "name": name,
                    "head_sha": MERGE,
                    "status": "completed",
                    "conclusion": "success",
                    "app": {"id": GITHUB_ACTIONS_APP},
                }
                for name in REQUIRED
            ],
            merge_evaluation={
                "mergeable": "MERGEABLE",
                "merge_state_status": "CLEAN",
                "blocked_by_expected_checks": False,
                "merge_commit_sha": MERGE,
            },
        )
        result = classify(data)
        self.assertEqual("COMPLETE_GREEN", result["verdict"])
        self.assertEqual(MERGE, result["subject_sha"])


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

    def test_conditional_required_jobs_are_the_known_native_conditions(self) -> None:
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
            if name == "ripr+ New Gap Gate":
                self.assert_ripr_native_finalizer(row, job)

    def assert_ripr_native_finalizer(self, row, job) -> None:
        # Recognizing a conditional job is not permission to weaken the
        # required native-G finalizer into any arbitrary predicate.
        self.assertTrue(row["required"])
        self.assertEqual(row["ruleset_integration_id"], GITHUB_ACTIONS_APP)
        self.assertEqual(row["workflow"], ".github/workflows/ripr.yml")
        self.assertEqual(row["job"], "ripr")
        self.assertEqual(job.condition, "always() && github.event_name == 'merge_group'")

    def test_ripr_known_condition_refuses_weakened_native_predicates(self) -> None:
        row = next(row for row in self.required_rows if row["name"] == "ripr+ New Gap Gate")
        job = self.workflows[row["workflow"]].jobs[row["job"]]
        self.assert_ripr_native_finalizer(row, job)
        for condition in (
            "github.event_name == 'merge_group'",
            "always()",
            "always() && github.event_name == 'workflow_dispatch'",
            "always() && github.event_name != 'pull_request'",
            "always() && github.event_name == 'merge_group' && false",
        ):
            with self.subTest(condition=condition):
                # Preserve the parser's conditional classification: merely
                # adding this context to the known set must not accept these.
                with self.assertRaises(AssertionError):
                    self.assert_ripr_native_finalizer(row, job._replace(condition=condition))
        for field, value in (("required", False), ("ruleset_integration_id", 999)):
            with self.subTest(field=field):
                with self.assertRaises(AssertionError):
                    self.assert_ripr_native_finalizer(dict(row, **{field: value}), job)

    def test_required_events_are_not_path_filtered(self) -> None:
        filtered = []
        for row in self.required_rows:
            workflow = self.workflows[row["workflow"]]
            for event in row.get("events", []):
                if event in workflow.path_filtered_events:
                    filtered.append((row["name"], event))
        self.assertEqual([], filtered)


class RecoveryContractTests(unittest.TestCase):
    def test_expected_stale_remedy_refuses_unauthorized_producers(self) -> None:
        remedy = detector.REMEDY["EXPECTED_STALE"]
        self.assertIn("Do not push an empty commit", remedy)
        self.assertIn("POST /repos/{owner}/{repo}/check-suites/{id}/rerequest", remedy)
        self.assertIn("does not count", remedy.lower())
        self.assertIn("#17900", remedy)
        self.assertIn("#17903", remedy)
        self.assertIn("NOT_PROVEN", remedy)

    def test_absent_remedy_does_not_treat_silence_as_success(self) -> None:
        remedy = detector.REMEDY["REQUIRED_ABSENT"]
        self.assertIn("never treat absence as success", remedy.lower())
        self.assertIn("#14612", remedy)
        self.assertIn("does not satisfy", remedy.lower())
        self.assertIn("#17900", remedy)

    def test_complete_green_remedy_is_subject_observation_not_protected_merge(self) -> None:
        remedy = detector.REMEDY["COMPLETE_GREEN"]
        self.assertIn("subject-observation", remedy)
        self.assertIn("test merge commit", remedy.lower())


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
