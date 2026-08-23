#!/usr/bin/env python3
"""Focused falsifiers for validate_gate_enforcement_contract.py."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("validate_gate_enforcement_contract.py")
SPEC = importlib.util.spec_from_file_location("gate_enforcement_contract", SCRIPT)
assert SPEC and SPEC.loader
contract = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = contract
SPEC.loader.exec_module(contract)

WORKFLOW = """
name: CI
on:
  pull_request:
  merge_group: {}
jobs:
  pr-smoke:
    name: PR Smoke (Fast Feedback, advisory)
    if: github.event_name == 'pull_request'
    runs-on: ubuntu-latest
    continue-on-error: true
    steps: []
  compile:
    name: Compile All Targets
    runs-on: ubuntu-latest
    steps: []
  internal-helper:
    name: Internal helper
    runs-on: ubuntu-latest
    steps: []
"""


def run_git(root: Path, *args: str) -> str:
    return subprocess.run(
        ["git", "-C", str(root), *args],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def commit_all(root: Path, message: str = "fixture") -> None:
    if not (root / ".git").exists():
        run_git(root, "init", "-q")
        run_git(root, "config", "user.name", "Test")
        run_git(root, "config", "user.email", "test@example.invalid")
    run_git(root, "add", "-A")
    run_git(root, "commit", "-q", "--allow-empty", "-m", message)


def policy_text(
    checks: str,
    *,
    version: int = 2,
    source: str = "github-enforcement-union",
) -> str:
    return (
        f'version = {version}\nsource = "{source}"\n\n'
        + textwrap.dedent(checks).lstrip()
    )


def fixture(
    root: Path,
    checks: str,
    *,
    workflow: str = WORKFLOW,
    version: int = 2,
    source: str = "github-enforcement-union",
) -> Path:
    path = root / ".ci/policies/required-checks.toml"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        policy_text(checks, version=version, source=source),
        encoding="utf-8",
    )
    ci = root / ".github/workflows/ci.yml"
    ci.parent.mkdir(parents=True, exist_ok=True)
    ci.write_text(textwrap.dedent(workflow).lstrip(), encoding="utf-8")
    commit_all(root)
    return path


REQUIRED = """
[[checks]]
name = "Compile All Targets"
producer = "repository-job"
workflow = ".github/workflows/ci.yml"
job = "compile"
workflow_result = "propagate"
events = ["pull_request", "merge_group"]
required = true
policy_role = "required"
applicability = "always-or-scoped-noop"
enforcement = "github-ruleset"
"""


class ContractTests(unittest.TestCase):
    def test_truthful_advisory_and_required_mappings_pass(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(
                root,
                """
                [[checks]]
                name = "PR Smoke (Fast Feedback, advisory)"
                producer = "repository-job"
                workflow = ".github/workflows/ci.yml"
                job = "pr-smoke"
                workflow_result = "continue"
                events = ["pull_request"]
                required = false
                policy_role = "advisory"
                applicability = "conditional"
                enforcement = "neither"

                """
                + REQUIRED,
            )
            result = contract.validate(root, policy)
        self.assertEqual("SUCCESS", result["status"])
        self.assertEqual(2, result["mapped_jobs"])

    def test_required_repository_context_requires_job_mapping(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(
                root,
                """
                [[checks]]
                name = "Compile All Targets"
                producer = "repository-job"
                workflow = ".github/workflows/ci.yml"
                workflow_result = "propagate"
                events = ["pull_request"]
                required = true
                policy_role = "required"
                applicability = "always-or-scoped-noop"
                enforcement = "github-ruleset"
                """,
            )
            result = contract.validate(root, policy)
        self.assertIn(
            "required_context_unmapped",
            {finding["code"] for finding in result["findings"]},
        )

    def test_required_context_cannot_continue_on_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(
                root,
                """
                [[checks]]
                name = "PR Smoke (Fast Feedback, advisory)"
                producer = "repository-job"
                workflow = ".github/workflows/ci.yml"
                job = "pr-smoke"
                workflow_result = "propagate"
                events = ["pull_request"]
                required = true
                policy_role = "required"
                applicability = "conditional"
                enforcement = "github-ruleset"
                """,
            )
            result = contract.validate(root, policy)
        self.assertIn(
            "workflow_result_mismatch",
            {finding["code"] for finding in result["findings"]},
        )

    def test_absent_commented_and_dynamic_continue_on_error_are_not_conflated(self) -> None:
        for value in ("# rationale", "${{ matrix.continue }}"):
            workflow = WORKFLOW.replace(
                "continue-on-error: true",
                f"continue-on-error: {value}",
            )
            with self.subTest(value=value), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                policy = fixture(
                    root,
                    """
                    [[checks]]
                    name = "PR Smoke (Fast Feedback, advisory)"
                    producer = "repository-job"
                    workflow = ".github/workflows/ci.yml"
                    job = "pr-smoke"
                    workflow_result = "continue"
                    events = ["pull_request"]
                    required = false
                    policy_role = "advisory"
                    applicability = "conditional"
                    enforcement = "neither"
                    """,
                    workflow=workflow,
                )
                result = contract.validate(root, policy)
            self.assertIn(
                "job_continue_on_error_not_static",
                {finding["code"] for finding in result["findings"]},
            )

    def test_quoted_job_control_keys_are_parsed(self) -> None:
        workflow = WORKFLOW.replace(
            "    if: github.event_name == 'pull_request'",
            '    "if": false',
        ).replace(
            "    continue-on-error: true",
            '    "continue-on-error": true',
        )
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(
                root,
                """
                [[checks]]
                name = "PR Smoke (Fast Feedback, advisory)"
                producer = "repository-job"
                workflow = ".github/workflows/ci.yml"
                job = "pr-smoke"
                workflow_result = "propagate"
                events = ["pull_request"]
                required = true
                policy_role = "required"
                applicability = "conditional"
                enforcement = "github-ruleset"
                """,
                workflow=workflow,
            )
            result = contract.validate(root, policy)
        codes = {finding["code"] for finding in result["findings"]}
        self.assertIn("job_unreachable", codes)
        self.assertIn("workflow_result_mismatch", codes)

    def test_constant_false_compound_condition_is_unreachable(self) -> None:
        for condition in (
            "${{ false && github.event_name == 'pull_request' }}",
            "${{ github.event_name == 'pull_request' && false }}",
            "${{ false || false }}",
            "${{ (false) && github.event_name == 'pull_request' }}",
            "${{ false || (false && github.event_name == 'push') }}",
        ):
            workflow = WORKFLOW.replace(
                "  compile:\n    name: Compile All Targets\n",
                f"  compile:\n    name: Compile All Targets\n    if: {condition}\n",
            )
            with self.subTest(condition=condition), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                policy = fixture(root, REQUIRED, workflow=workflow)
                result = contract.validate(root, policy)
            self.assertIn(
                "job_unreachable",
                {finding["code"] for finding in result["findings"]},
            )

    def test_context_dependent_conditions_are_not_folded_to_unreachable(self) -> None:
        """Constant folding must not over-reach and falsely block a live gate.

        Each expression depends on context this bounded parser does not
        evaluate, so it must stay `conditional` rather than becoming `never`.
        The label case is the discriminating one: a `false` substring inside a
        quoted string is not a boolean literal.
        """
        for condition in (
            "${{ github.event_name == 'pull_request' }}",
            "${{ false || github.event_name == 'push' }}",
            "${{ github.a && github.b }}",
            "${{ !cancelled() }}",
            "${{ contains(github.event.pull_request.labels.*.name, 'ci:false') }}",
        ):
            with self.subTest(condition=condition):
                self.assertEqual("conditional", contract._condition_class(condition))

    def test_advisory_role_cannot_claim_required_enforcement(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(
                root,
                """
                [[checks]]
                name = "PR Smoke (Fast Feedback, advisory)"
                producer = "repository-job"
                workflow = ".github/workflows/ci.yml"
                job = "pr-smoke"
                workflow_result = "continue"
                events = ["pull_request"]
                required = true
                policy_role = "advisory"
                applicability = "conditional"
                enforcement = "github-ruleset"
                """,
            )
            result = contract.validate(root, policy)
        codes = {finding["code"] for finding in result["findings"]}
        self.assertIn("role_required_mismatch", codes)
        self.assertIn("nonrequired_claims_github_enforcement", codes)

    def test_mapped_name_must_match_static_job_name(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(
                root,
                """
                [[checks]]
                name = "PR Smoke"
                producer = "repository-job"
                workflow = ".github/workflows/ci.yml"
                job = "pr-smoke"
                workflow_result = "continue"
                events = ["pull_request"]
                required = false
                policy_role = "advisory"
                applicability = "conditional"
                enforcement = "neither"
                """,
            )
            result = contract.validate(root, policy)
        self.assertIn(
            "context_name_mismatch",
            {finding["code"] for finding in result["findings"]},
        )

    def test_absent_job_name_uses_static_job_id(self) -> None:
        workflow = WORKFLOW.replace("    name: Compile All Targets\n", "")
        policy_value = REQUIRED.replace(
            'name = "Compile All Targets"',
            'name = "compile"',
        )
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(root, policy_value, workflow=workflow)
            result = contract.validate(root, policy)
        self.assertEqual("SUCCESS", result["status"])

    def test_unlisted_internal_job_does_not_become_merge_policy(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(root, REQUIRED)
            result = contract.validate(root, policy)
        self.assertEqual("SUCCESS", result["status"])

    def test_condition_and_event_claims_are_verified(self) -> None:
        workflow = WORKFLOW.replace(
            "  merge_group: {}\n",
            "  merge_group:\n    paths: [\"src/**\"]\n",
        ).replace(
            "  compile:\n    name: Compile All Targets\n",
            "  compile:\n    name: Compile All Targets\n    if: false\n",
        )
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(root, REQUIRED, workflow=workflow)
            result = contract.validate(root, policy)
        codes = {finding["code"] for finding in result["findings"]}
        self.assertIn("job_unreachable", codes)
        self.assertIn("required_event_path_filtered", codes)

    def test_conditional_applicability_must_match_observed_job_condition(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(
                root,
                REQUIRED.replace(
                    'applicability = "always-or-scoped-noop"',
                    'applicability = "conditional"',
                ),
            )
            result = contract.validate(root, policy)
        self.assertIn(
            "applicability_mismatch",
            {finding["code"] for finding in result["findings"]},
        )

    def test_workflow_path_rejects_parent_traversal_and_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(
                root,
                REQUIRED.replace(
                    'workflow = ".github/workflows/ci.yml"',
                    'workflow = "../outside.yml"',
                ),
            )
            result = contract.validate(root, policy)
        self.assertIn(
            "workflow_missing",
            {finding["code"] for finding in result["findings"]},
        )

        if hasattr(os, "symlink"):
            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                policy = fixture(root, REQUIRED)
                outside = root / "outside.yml"
                outside.write_text(
                    textwrap.dedent(WORKFLOW).lstrip(),
                    encoding="utf-8",
                )
                link = root / ".github/workflows/ci.yml"
                link.unlink()
                link.symlink_to(outside)
                commit_all(root, "symlink")
                with self.assertRaisesRegex(ValueError, "traverses a symlink"):
                    contract.validate(root, policy)

    def test_duplicate_static_emitter_is_detected_even_when_unlisted(self) -> None:
        workflow = WORKFLOW.replace(
            "  internal-helper:\n    name: Internal helper\n",
            "  internal-helper:\n    name: Compile All Targets\n",
        )
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(root, REQUIRED, workflow=workflow)
            result = contract.validate(root, policy)
        self.assertIn(
            "duplicate_emitted_context",
            {finding["code"] for finding in result["findings"]},
        )

    def test_external_producer_is_explicit_and_has_no_repository_job(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(
                root,
                """
                [[checks]]
                name = "codecov/patch"
                producer = "external"
                workflow = "codecov"
                required = false
                policy_role = "informational"
                applicability = "conditional"
                enforcement = "neither"
                """,
            )
            result = contract.validate(root, policy)
        self.assertEqual("SUCCESS", result["status"])
        self.assertEqual(0, result["mapped_jobs"])

    def test_receipt_is_bound_to_repo_policy_workflows_and_contexts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(root, REQUIRED)
            result = contract.validate(root, policy)
            repository_sha = run_git(root, "rev-parse", "HEAD")
            policy_digest = hashlib.sha256(policy.read_bytes()).hexdigest()
            workflow = root / ".github/workflows/ci.yml"
            workflow_digest = hashlib.sha256(workflow.read_bytes()).hexdigest()
        self.assertEqual(repository_sha, result["subjects"]["repository_sha"])
        self.assertFalse(result["subjects"]["repository_dirty"])
        self.assertEqual(
            policy_digest,
            result["subjects"]["policy"]["sha256"],
        )
        self.assertIn(
            {".github/workflows/ci.yml": workflow_digest},
            [
                {entry["path"]: entry["sha256"]}
                for entry in result["subjects"]["workflow_catalog"]
            ],
        )
        self.assertRegex(result["subject_sha256"], r"^[0-9a-f]{64}$")

    def test_unsupported_policy_version_and_source_are_not_proven(self) -> None:
        for version, source in (
            (3, "github-enforcement-union"),
            (2, "copied-list"),
        ):
            with (
                self.subTest(version=version, source=source),
                tempfile.TemporaryDirectory() as tmp,
            ):
                root = Path(tmp)
                policy = fixture(
                    root,
                    REQUIRED,
                    version=version,
                    source=source,
                )
                with self.assertRaisesRegex(ValueError, "unsupported policy"):
                    contract.validate(root, policy)

    def test_bad_policy_writes_not_proven_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".github/workflows").mkdir(parents=True)
            (root / ".github/workflows/ci.yml").write_text(
                textwrap.dedent(WORKFLOW).lstrip(),
                encoding="utf-8",
            )
            bad = root / ".ci/policies/required-checks.toml"
            bad.parent.mkdir(parents=True)
            bad.write_text("[[checks]\n", encoding="utf-8")
            commit_all(root)
            receipt = root / "receipt.json"
            status = contract.main(
                [
                    "--root",
                    str(root),
                    "--policy",
                    str(bad),
                    "--receipt",
                    str(receipt),
                ]
            )
            payload = json.loads(receipt.read_text(encoding="utf-8"))
        self.assertEqual(1, status)
        self.assertEqual("NOT_PROVEN", payload["status"])
        self.assertEqual("NOT_PROVEN", payload["live_enforcement_status"])
        self.assertIsNone(payload["subject_sha256"])

    def test_empty_checks_collection_fails_closed(self) -> None:
        """An explicit `checks = []` must not report a vacuous SUCCESS.

        A prior version of the validator only checked
        ``isinstance(contexts, list)`` and accepted an empty list,
        producing ``status: SUCCESS`` with zero contexts validated and a
        real bound ``subject_sha256`` -- the exact dishonest vacuous-pass
        class this contract exists to eliminate.
        """
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(root, "checks = []\n")
            with self.assertRaisesRegex(ValueError, "at least one"):
                contract.validate(root, policy)

    def test_empty_checks_collection_writes_not_proven_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(root, "checks = []\n")
            receipt = root / "receipt.json"
            status = contract.main(
                [
                    "--root",
                    str(root),
                    "--policy",
                    str(policy),
                    "--receipt",
                    str(receipt),
                ]
            )
            payload = json.loads(receipt.read_text(encoding="utf-8"))
        self.assertEqual(1, status)
        self.assertNotEqual("SUCCESS", payload["status"])
        self.assertEqual(0, payload["contexts"])

    def test_empty_events_list_fails_closed(self) -> None:
        """A repository-job context with `events = []` must not silently
        skip event-reachability checks. The same empty-collection hole
        exists in this sibling per-context list; it must fail closed with
        a named finding rather than a silent no-op.
        """
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(
                root,
                REQUIRED.replace(
                    'events = ["pull_request", "merge_group"]',
                    "events = []",
                ),
            )
            result = contract.validate(root, policy)
        self.assertEqual("BLOCKED", result["status"])
        self.assertIn(
            "invalid_events",
            {finding["code"] for finding in result["findings"]},
        )

    def test_unknown_context_field_fails_closed_with_actionable_identity(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = fixture(
                root,
                REQUIRED.replace(
                    'enforcement = "github-ruleset"',
                    'enforcement = "github-ruleset"\nenforcemnt = "github-branch-protection"',
                ),
            )
            result = contract.validate(root, policy)
        finding = next(
            finding
            for finding in result["findings"]
            if finding["code"] == "unknown_context_field"
        )
        self.assertEqual("BLOCKED", result["status"])
        self.assertEqual("Compile All Targets", finding["subject"])
        self.assertIn("enforcemnt", finding["message"])

    def test_context_source_order_does_not_move_semantic_subject(self) -> None:
        advisory = """
        [[checks]]
        name = "codecov/patch"
        producer = "external"
        workflow = "codecov"
        required = false
        policy_role = "informational"
        applicability = "conditional"
        enforcement = "neither"
        reason = "External telemetry is not merge authority."
        """
        with (
            tempfile.TemporaryDirectory() as left_tmp,
            tempfile.TemporaryDirectory() as right_tmp,
        ):
            left_root = Path(left_tmp)
            right_root = Path(right_tmp)
            left = contract.validate(
                left_root,
                fixture(left_root, advisory + REQUIRED),
            )
            right = contract.validate(
                right_root,
                fixture(right_root, REQUIRED + advisory),
            )
        self.assertEqual("SUCCESS", left["status"])
        self.assertEqual("SUCCESS", right["status"])
        self.assertEqual(left["semantic_subject"], right["semantic_subject"])
        self.assertEqual(left["subject_sha256"], right["subject_sha256"])
        self.assertNotEqual(
            left["subjects"]["policy"]["sha256"],
            right["subjects"]["policy"]["sha256"],
        )
        self.assertNotEqual(
            left["exact_source_sha256"],
            right["exact_source_sha256"],
        )

    def test_binding_movement_changes_semantic_subject(self) -> None:
        bound = REQUIRED.replace(
            'enforcement = "github-ruleset"',
            'enforcement = "github-ruleset"\nruleset_integration_id = 15368',
        )
        with (
            tempfile.TemporaryDirectory() as left_tmp,
            tempfile.TemporaryDirectory() as right_tmp,
        ):
            left_root = Path(left_tmp)
            right_root = Path(right_tmp)
            left = contract.validate(left_root, fixture(left_root, REQUIRED))
            right = contract.validate(right_root, fixture(right_root, bound))
        self.assertEqual("SUCCESS", left["status"])
        self.assertEqual("SUCCESS", right["status"])
        self.assertNotEqual(left["subject_sha256"], right["subject_sha256"])


if __name__ == "__main__":
    unittest.main()
