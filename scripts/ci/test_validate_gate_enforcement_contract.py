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

# Live repository surfaces for the #10536 A2 prerequisites. Unlike the
# fixture-based falsifiers above, the tests below read checked-in workflow
# files directly (test_rustfmt_required_workflow.py precedent) so a shape
# regression fails this lane instead of being rediscovered at ruleset
# registration time.
ROOT = Path(__file__).resolve().parents[2]
RATCHET_WORKFLOW_RELPATH = ".github/workflows/workflow-security-ratchet.yml"
RATCHET_JOB_ID = "trusted-base-ratchet"
RATCHET_CONTEXT_NAME = "Trusted-base workflow security ratchet"
MAIN_SELFCHECK_CONDITION = (
    "github.event_name == 'push' || github.event_name == 'workflow_dispatch'"
)

# Verbatim fields of the registry row proposed for post-registration landing:
# issue #10536, proposal comment 5431630603 (2026-08-26T22:01:53Z), Option A1.
# The row is intentionally absent from .ci/policies/required-checks.toml until
# owner step A2 makes its enforcement claim true (residual 1/2 in disposition
# comment 5433849110); validity against the live catalog is pinned here so the
# post-A2 reconciliation lands it verbatim instead of re-deriving it.
TRUSTED_BASE_DRAFT_ROW = {
    "name": RATCHET_CONTEXT_NAME,
    "producer": "repository-job",
    "workflow": RATCHET_WORKFLOW_RELPATH,
    "job": RATCHET_JOB_ID,
    "workflow_result": "propagate",
    "events": ["pull_request_target"],
    "required": True,
    "policy_role": "required",
    "applicability": "always-or-scoped-noop",
    "enforcement": "github-ruleset",
    # Single intentional deviation from proposal comment 5431630603: the
    # reason is phrased as post-A2 rather than asserting current live
    # enforcement, which does not exist until the owner registers the
    # context on ruleset 16664791. Every classification-relevant field is
    # verbatim; this validator ignores `reason` wording.
    "reason": (
        "Main ruleset (id 16664791) will require the trusted-base workflow "
        "security ratchet before merge once registration A2 executes (#10536), "
        "closing the gap where workflow-touching dependency PRs cannot land "
        "while pin rotation creates new baseline findings."
    ),
}

RESOLVER_STEP = "Resolve trusted-base scope"
CHECKOUT_STEP = "Checkout exact trusted base authority"
IMPORT_STEP = "Import exact PR head as inert Git data"
EVALUATOR_PROOF_STEP = "Prove trusted evaluator before use"
RATCHET_CHECK_STEP = "Reject new findings against the trusted base baseline"
BASELINE_VALIDATE_STEP = "Reject stale, pre-seeded, or tampered candidate baseline"
RECEIPT_UPLOAD_STEP = "Upload authoritative ratchet receipts"

EXPECTED_JOB_STEPS = (
    RESOLVER_STEP,
    CHECKOUT_STEP,
    IMPORT_STEP,
    EVALUATOR_PROOF_STEP,
    RATCHET_CHECK_STEP,
    BASELINE_VALIDATE_STEP,
    RECEIPT_UPLOAD_STEP,
)
# Scan-path steps only execute while the run is still healthy.
GUARDED_STEPS = (
    CHECKOUT_STEP,
    IMPORT_STEP,
    EVALUATOR_PROOF_STEP,
    RATCHET_CHECK_STEP,
    BASELINE_VALIDATE_STEP,
)
UPLOAD_GUARD = "${{ !cancelled() && env.SCOPED_NOOP != 'true' }}"


def _repo_text(relative: str) -> str:
    return ROOT.joinpath(*relative.split("/")).read_text(encoding="utf-8")


_LIVE_CATALOG: tuple[dict, dict] | None = None


def _live_catalog() -> tuple[dict, dict]:
    """Build the real workflow catalog and producer index once per process.

    ``read_workflow`` shells out to git per tracked workflow file, so
    rebuilding the catalog in every test method multiplies into hundreds of
    subprocesses on Windows runners.
    """
    global _LIVE_CATALOG
    if _LIVE_CATALOG is None:
        workflows = contract.read_workflow_catalog(ROOT)
        _LIVE_CATALOG = (workflows, contract.build_producer_index(workflows))
    return _LIVE_CATALOG


def _split_steps(job_block_lines: list[str]) -> dict[str, str]:
    """Map each step's `name:` value to its chunk text.

    The boundary is the repo-standard `- name: ` list item at six-space
    indent inside a job block; anything above that indentation belongs to the
    next chunk. Drift to another layout must fail this helper loudly rather
    than pass vacuously.
    """
    starts: list[tuple[int, str]] = []
    for index, line in enumerate(job_block_lines):
        if line.startswith("      - name: "):
            starts.append((index, line[len("      - name: ") :].strip()))
    if [name for _, name in starts] != list(EXPECTED_JOB_STEPS):
        raise AssertionError(
            "trusted-base-ratchet steps drifted from pinned inventory: "
            f"{[name for _, name in starts]!r}"
        )
    chunks: dict[str, str] = {}
    for position, (start, name) in enumerate(starts):
        end = starts[position + 1][0] if position + 1 < len(starts) else len(
            job_block_lines
        )
        chunks[name] = "\n".join(job_block_lines[start:end])
    return chunks


def _job_direct_children(job_block_lines: list[str]) -> list[str]:
    children: list[str] = []
    for line in job_block_lines[1:]:
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        indent = len(line) - len(line.lstrip(" "))
        if indent < 4:
            break
        if indent == 4 and ":" in line:
            children.append(line.strip().split(":", 1)[0].strip().strip("'\""))
    return children


class TrustedBaseRatchetRequiredShapeTests(unittest.TestCase):
    """Live-catalog falsifiers binding the #10536 residual (#12845 follow-up).

    Owner registration step A2 requires the proposed required context to
    satisfy ``applicability == always-or-scoped-noop``, which pins
    ``condition_class == always`` on the emitting job. These tests keep the
    three surfaces honest together: the real workflow, this validator's
    classification, and the deferred verbatim [[checks]] row.
    """

    def setUp(self) -> None:
        self.workflows, self.producers = _live_catalog()
        self.ratchet = self.workflows[RATCHET_WORKFLOW_RELPATH]
        self.job = self.ratchet.jobs[RATCHET_JOB_ID]
        lines = _repo_text(RATCHET_WORKFLOW_RELPATH).splitlines()
        start = lines.index(f"  {RATCHET_JOB_ID}:")
        end = lines.index("  main-selfcheck:")
        self.job_block = lines[start:end]

    def test_verbatim_10536_row_maps_clean_against_live_catalog(self) -> None:
        findings, mapped = contract.validate_context(
            TRUSTED_BASE_DRAFT_ROW,
            self.workflows,
            self.producers,
        )
        self.assertEqual([], findings)
        self.assertTrue(mapped)

    def test_emitting_job_is_unconditionally_reachable(self) -> None:
        self.assertIsNone(
            self.job.condition,
            "job-level event gate returned; applicability filtering must stay in-job",
        )
        self.assertEqual("always", self.job.condition_class)
        self.assertFalse(self.job.continue_on_error)
        self.assertTrue(self.job.continue_static)

    def test_context_identity_is_unique_and_static(self) -> None:
        self.assertEqual(RATCHET_CONTEXT_NAME, self.job.name)
        self.assertTrue(self.job.name_static)
        producers = self.producers.get(RATCHET_CONTEXT_NAME, [])
        self.assertEqual(
            [(RATCHET_WORKFLOW_RELPATH, RATCHET_JOB_ID)],
            producers,
        )

    def test_declared_event_reachability_holds_for_pull_request_target(self) -> None:
        self.assertIn("pull_request_target", self.ratchet.events)
        self.assertNotIn("pull_request_target", self.ratchet.path_filtered_events)

    def test_all_filtering_resolves_in_job_not_at_job_level(self) -> None:
        children = _job_direct_children(self.job_block)
        self.assertNotIn("if", children)
        steps = _split_steps(self.job_block)
        self.assertNotIn("if:", steps[RESOLVER_STEP])
        for name in GUARDED_STEPS:
            self.assertIn(
                "if: env.SCOPED_NOOP != 'true'",
                steps[name],
                f"step {name!r} lost its scoped guard",
            )

    def test_receipt_upload_survives_scan_failure(self) -> None:
        """A bare non-status `if` gets an implicit `success()` prefix.

        The receipt upload must keep an explicit status function so failing
        validation steps still upload their diagnostic receipts; this also
        keeps the scoped-noop skip for runs that never scanned.
        """
        chunks = _split_steps(self.job_block)
        self.assertIn(
            f"if: {UPLOAD_GUARD}",
            chunks[RECEIPT_UPLOAD_STEP],
        )

    def test_scope_resolver_delegates_non_pr_events_via_env_indirection(self) -> None:
        steps = _split_steps(self.job_block)
        resolver = steps[RESOLVER_STEP]
        self.assertIn('EVENT_NAME: ${{ github.event_name }}', resolver)
        self.assertEqual(1, resolver.count("${{"))
        self.assertIn('"$EVENT_NAME" != "pull_request_target"', resolver)
        self.assertIn("SCOPED_NOOP=true", resolver)
        # Delegation target must remain real: main-selfcheck still owns
        # push / workflow_dispatch validation of the trusted base.
        sibling = self.ratchet.jobs["main-selfcheck"]
        self.assertEqual(MAIN_SELFCHECK_CONDITION, sibling.condition)
        self.assertIn("push", self.ratchet.events)

    def test_reintroduced_job_level_event_gate_reddens_the_draft_row(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workflow_dir = root / ".github/workflows"
            workflow_dir.mkdir(parents=True)
            regressed = _repo_text(RATCHET_WORKFLOW_RELPATH).replace(
                f"  {RATCHET_JOB_ID}:\n",
                f"  {RATCHET_JOB_ID}:\n"
                "    if: github.event_name == 'pull_request_target'\n",
                1,
            )
            (workflow_dir / "workflow-security-ratchet.yml").write_text(
                regressed, encoding="utf-8"
            )
            commit_all(root, "regression fixture")
            workflows = contract.read_workflow_catalog(root)
            job = workflows[RATCHET_WORKFLOW_RELPATH].jobs[RATCHET_JOB_ID]
            self.assertEqual("conditional", job.condition_class)
            findings, _ = contract.validate_context(
                TRUSTED_BASE_DRAFT_ROW,
                workflows,
                contract.build_producer_index(workflows),
            )
        self.assertIn(
            "applicability_mismatch",
            {finding.code for finding in findings},
        )


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
