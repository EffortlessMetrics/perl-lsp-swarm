#!/usr/bin/env python3
"""Structural contracts for the standalone Rust formatting context."""

from __future__ import annotations

import copy
import tomllib
import unittest
from pathlib import Path
from typing import Any

import yaml


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "ci.yml"
POLICY_PATH = ROOT / ".ci" / "policies" / "required-checks.toml"
JOB_ID = "rust-formatting"
CONTEXT_NAME = "Rust formatting"
RUN_STEP = "Run candidate-bound rustfmt check"
VERIFY_STEP = "Verify candidate-bound rustfmt receipt"
UPLOAD_STEP = "Upload candidate-bound rustfmt receipt"
SUBJECT_EXPRESSION = (
    "github.event_name == 'pull_request' && github.event.pull_request.head.sha || "
    "github.event_name == 'merge_group' && github.event.merge_group.head_sha || github.sha"
)


def load_workflow() -> dict[str, Any]:
    payload = yaml.load(WORKFLOW_PATH.read_text(encoding="utf-8"), Loader=yaml.BaseLoader)
    if not isinstance(payload, dict):
        raise AssertionError("CI workflow must parse as a mapping")
    return payload


def load_policy() -> dict[str, object]:
    return tomllib.loads(POLICY_PATH.read_text(encoding="utf-8"))


def named_step(job: dict[str, Any], name: str) -> dict[str, Any]:
    matches = [item for item in job.get("steps", []) if item.get("name") == name]
    if len(matches) != 1:
        raise AssertionError(f"expected exactly one {name!r} step, found {len(matches)}")
    return matches[0]


def validate_contract(workflow: dict[str, Any], policy: dict[str, object]) -> None:
    triggers = workflow.get("on", {})
    for event in ("pull_request", "merge_group", "push"):
        if event not in triggers:
            raise AssertionError(f"workflow must report on {event}")
    pull_request = triggers.get("pull_request", {})
    if "paths" in pull_request or "paths-ignore" in pull_request:
        raise AssertionError("formatter context must remain terminal for docs-only PRs")

    job = workflow.get("jobs", {}).get(JOB_ID)
    if not isinstance(job, dict):
        raise AssertionError("formatter job is missing")
    if job.get("name") != CONTEXT_NAME:
        raise AssertionError("formatter context name drifted")
    if job.get("continue-on-error") == "true":
        raise AssertionError("formatter job must fail on formatter or instrument failure")
    if "needs" in job or "if" in job:
        raise AssertionError("formatter context must run terminally on every triggered subject")
    if job.get("env", {}).get("SUBJECT_SHA") != "${{ " + SUBJECT_EXPRESSION + " }}":
        raise AssertionError("formatter subject must bind PR, merge-group, and push commits exactly")

    checkout = named_step(job, "Checkout exact formatter subject")
    checkout_with = checkout.get("with", {})
    if checkout_with.get("ref") != "${{ env.SUBJECT_SHA }}":
        raise AssertionError("checkout must use the exact formatter subject")
    if checkout_with.get("persist-credentials") != "false":
        raise AssertionError("formatter checkout must not persist workflow credentials")

    install = named_step(job, "Install pinned formatter toolchain")
    install_with = install.get("with", {})
    if install_with.get("toolchain") != "1.95.0" or install_with.get("components") != "rustfmt":
        raise AssertionError("formatter must remain pinned to Rust 1.95.0 rustfmt")

    run_step = named_step(job, RUN_STEP)
    if run_step.get("continue-on-error") == "true":
        raise AssertionError("formatter run step must not continue on error")
    run = run_step.get("run", "")
    for required_fragment in (
        "scripts/ci/rustfmt_check.py",
        '--candidate-sha "$SUBJECT_SHA"',
        '--candidate-tree-sha "$SUBJECT_TREE_SHA"',
        "rustup which --toolchain 1.95.0 rustfmt",
        "rustup which --toolchain 1.95.0 rustc",
        '--rustc "$rustc_bin"',
    ):
        if required_fragment not in run:
            raise AssertionError(f"formatter invocation missing {required_fragment!r}")
    if "|| true" in run:
        raise AssertionError("formatter failure must not be ignored")

    verify_step = named_step(job, VERIFY_STEP)
    if verify_step.get("continue-on-error") == "true":
        raise AssertionError("receipt verifier must not continue on error")
    verify_run = verify_step.get("run", "")
    for required_fragment in (
        "scripts/ci/verify_rustfmt_receipt.py",
        '--receipt "$RECEIPT_PATH"',
        "--producer scripts/ci/rustfmt_check.py",
        '--candidate-sha "$SUBJECT_SHA"',
        '--candidate-tree-sha "$SUBJECT_TREE_SHA"',
        '--rustc "$rustc_bin"',
    ):
        if required_fragment not in verify_run:
            raise AssertionError(f"receipt verifier missing {required_fragment!r}")
    if "|| true" in verify_run:
        raise AssertionError("receipt verifier must not be bypassed")
    steps = job.get("steps", [])
    if steps.index(verify_step) <= steps.index(run_step) or steps.index(verify_step) >= steps.index(named_step(job, UPLOAD_STEP)):
        raise AssertionError("receipt verifier must run after producer and before upload")

    upload = named_step(job, UPLOAD_STEP)
    upload_with = upload.get("with", {})
    if upload.get("if") != "always()":
        raise AssertionError("formatter receipt upload must always run")
    if upload_with.get("name") != "rustfmt-check-${{ env.SUBJECT_SHA }}":
        raise AssertionError("formatter artifact name must bind the exact subject")
    if upload_with.get("path") != "${{ env.RECEIPT_PATH }}":
        raise AssertionError("formatter artifact path must use the receipt path")
    if upload_with.get("if-no-files-found") != "error":
        raise AssertionError("missing formatter receipt must fail closed")

    entries = [item for item in policy.get("checks", []) if item.get("name") == CONTEXT_NAME]
    if len(entries) != 1:
        raise AssertionError("policy must contain exactly one formatter context")
    entry = entries[0]
    if entry.get("workflow") != ".github/workflows/ci.yml":
        raise AssertionError("formatter policy must name the owning workflow")
    if entry.get("required") is not True or entry.get("enforcement") != "github-ruleset":
        raise AssertionError("formatter policy must match live GitHub ruleset enforcement")
    if "16664791" not in str(entry.get("reason", "")):
        raise AssertionError("formatter policy must name the live ruleset authority")


class RustfmtRequiredWorkflowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = load_workflow()
        self.policy = load_policy()

    def job(self, workflow: dict[str, Any]) -> dict[str, Any]:
        return workflow["jobs"][JOB_ID]

    def test_checked_in_workflow_and_policy_are_coherent(self) -> None:
        validate_contract(self.workflow, self.policy)

    def test_missing_merge_group_subject_fails_closed(self) -> None:
        broken = copy.deepcopy(self.workflow)
        self.job(broken)["env"]["SUBJECT_SHA"] = "${{ github.sha }}"
        with self.assertRaisesRegex(AssertionError, "bind PR, merge-group, and push"):
            validate_contract(broken, self.policy)

    def test_run_step_cannot_continue_on_error(self) -> None:
        broken = copy.deepcopy(self.workflow)
        named_step(self.job(broken), RUN_STEP)["continue-on-error"] = "true"
        with self.assertRaisesRegex(AssertionError, "run step must not continue"):
            validate_contract(broken, self.policy)

    def test_verifier_cannot_be_removed_or_bypassed(self) -> None:
        broken = copy.deepcopy(self.workflow)
        self.job(broken)["steps"] = [step for step in self.job(broken)["steps"] if step.get("name") != VERIFY_STEP]
        with self.assertRaisesRegex(AssertionError, "expected exactly one"):
            validate_contract(broken, self.policy)
        broken = copy.deepcopy(self.workflow)
        named_step(self.job(broken), VERIFY_STEP)["continue-on-error"] = "true"
        with self.assertRaisesRegex(AssertionError, "verifier must not continue"):
            validate_contract(broken, self.policy)
        broken = copy.deepcopy(self.workflow)
        named_step(self.job(broken), VERIFY_STEP)["run"] += "\ntrue || true"
        with self.assertRaisesRegex(AssertionError, "verifier must not be bypassed"):
            validate_contract(broken, self.policy)

    def test_wrong_artifact_name_is_rejected(self) -> None:
        broken = copy.deepcopy(self.workflow)
        named_step(self.job(broken), UPLOAD_STEP)["with"]["name"] = "rustfmt-check-latest"
        with self.assertRaisesRegex(AssertionError, "artifact name"):
            validate_contract(broken, self.policy)

    def test_wrong_artifact_path_is_rejected(self) -> None:
        broken = copy.deepcopy(self.workflow)
        named_step(self.job(broken), UPLOAD_STEP)["with"]["path"] = "target/other.json"
        with self.assertRaisesRegex(AssertionError, "artifact path"):
            validate_contract(broken, self.policy)

    def test_upload_without_always_is_rejected_despite_decoy(self) -> None:
        broken = copy.deepcopy(self.workflow)
        job = self.job(broken)
        named_step(job, UPLOAD_STEP).pop("if")
        job["steps"].append({"name": "Decoy always", "if": "always()", "run": "true"})
        with self.assertRaisesRegex(AssertionError, "upload must always run"):
            validate_contract(broken, self.policy)

    def test_missing_receipt_cannot_report_green(self) -> None:
        broken = copy.deepcopy(self.workflow)
        named_step(self.job(broken), UPLOAD_STEP)["with"]["if-no-files-found"] = "warn"
        with self.assertRaisesRegex(AssertionError, "must fail closed"):
            validate_contract(broken, self.policy)

    def test_advisory_policy_is_rejected_after_promotion(self) -> None:
        broken = copy.deepcopy(self.policy)
        entry = next(item for item in broken["checks"] if item.get("name") == CONTEXT_NAME)
        entry["required"] = False
        entry["enforcement"] = "advisory"
        with self.assertRaisesRegex(AssertionError, "match live GitHub ruleset"):
            validate_contract(self.workflow, broken)


if __name__ == "__main__":
    unittest.main()
