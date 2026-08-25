#!/usr/bin/env python3
"""Structural contracts for rustfmt prevention: advisory producer + required Rust Small path."""

from __future__ import annotations

import copy
import importlib.util
import re
import sys
import tomllib
import unittest
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
YAML_HELPER = ROOT / "scripts" / "ci" / "workflow_security_ratchet.py"
SPEC = importlib.util.spec_from_file_location("workflow_security_ratchet", YAML_HELPER)
assert SPEC and SPEC.loader
yaml_structure = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = yaml_structure
SPEC.loader.exec_module(yaml_structure)

WORKFLOW_PATH = ROOT / ".github" / "workflows" / "ci.yml"
RUST_SMALL_WORKFLOW_PATH = ROOT / ".github" / "workflows" / "em-ci-routed-rust.yml"
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
RUST_SMALL_LANE_JOBS = (
    "rust-small-cx53",
    "rust-small-cx43",
    "rust-small-github",
    "rust-small-fallback",
)
RUST_SMALL_RESULT_JOB = "rust-small-result"
FMT_COMMAND = "cargo fmt --all -- --check"
CONTRACT_TEST_FILES = (
    "scripts/ci/test_rustfmt_check.py",
    "scripts/ci/test_rustfmt_required_workflow.py",
)
CARGO_FMT_RE = re.compile(r"cargo\s+fmt\b")


def load_workflow() -> dict[str, Any]:
    lines = WORKFLOW_PATH.read_text(encoding="utf-8").splitlines()
    triggers: dict[str, object] = {}
    job: dict[str, Any] = {"env": {}, "steps": []}
    section = ""
    current_step: dict[str, Any] | None = None
    nested: dict[str, str] | None = None
    index = 0
    while index < len(lines):
        parsed = yaml_structure._parse_key_line(lines[index])
        if parsed and parsed.indent == 0 and parsed.key in {"on", "jobs"}:
            section = parsed.key
        elif section == "on" and parsed and parsed.indent == 2:
            triggers[parsed.key] = {}
        elif section in {"jobs", "other-job"} and parsed and parsed.indent == 2:
            section = "formatter" if parsed.key == JOB_ID else "other-job"
        elif section == "formatter" and parsed:
            value = yaml_structure._strip_scalar(parsed.value)
            if parsed.list_item and parsed.indent == 8 and parsed.key == "name":
                current_step = {"name": value}
                job["steps"].append(current_step)
                nested = None
            elif parsed.indent == 4:
                if parsed.key not in {"env", "steps"}:
                    job[parsed.key] = value
                current_step = None
                nested = None
            elif parsed.indent == 6 and current_step is None:
                job["env"][parsed.key] = value
            elif parsed.indent == 8 and current_step is not None:
                if parsed.key in {"with"}:
                    nested = {}
                    current_step[parsed.key] = nested
                elif parsed.key == "run" and value in {"|", ">"}:
                    block: list[str] = []
                    cursor = index + 1
                    while cursor < len(lines):
                        candidate = lines[cursor]
                        if candidate.strip() and len(candidate) - len(candidate.lstrip()) <= 8:
                            break
                        block.append(candidate[10:] if len(candidate) >= 10 else "")
                        cursor += 1
                    current_step["run"] = "\n".join(block)
                    index = cursor - 1
                else:
                    current_step[parsed.key] = value
                    nested = None
            elif parsed.indent == 10 and current_step is not None and nested is not None:
                nested[parsed.key] = value
        index += 1
    return {"on": triggers, "jobs": {JOB_ID: job}}


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
    if entry.get("job") != JOB_ID:
        raise AssertionError("formatter policy must name the owning job")
    # Policy schema v2 separates merge-policy role from live GitHub enforcement.
    # "advisory" is a `policy_role`; the enforcement source must stay unclaimed
    # until a reviewed settings change actually protects the context.
    if (
        entry.get("required") is not False
        or entry.get("policy_role") != "advisory"
        or entry.get("enforcement") != "neither"
    ):
        raise AssertionError("formatter policy must remain advisory before post-merge promotion")
    if "post-merge promotion target" not in str(entry.get("reason", "")).lower():
        raise AssertionError("formatter policy must retain the post-merge promotion target")


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

    def test_premature_required_policy_is_rejected(self) -> None:
        broken = copy.deepcopy(self.policy)
        entry = next(item for item in broken["checks"] if item.get("name") == CONTEXT_NAME)
        entry["required"] = True
        entry["policy_role"] = "required"
        entry["enforcement"] = "github-ruleset"
        with self.assertRaisesRegex(AssertionError, "remain advisory"):
            validate_contract(self.workflow, broken)


def load_rust_small_workflow_text() -> str:
    return RUST_SMALL_WORKFLOW_PATH.read_text(encoding="utf-8")


def job_bodies(workflow_text: str) -> dict[str, str]:
    """Return indent-2 GitHub Actions job bodies keyed by job id."""
    bodies: dict[str, list[str]] = {}
    current: str | None = None
    in_jobs = False
    for line in workflow_text.splitlines():
        if line == "jobs:":
            in_jobs = True
            current = None
            continue
        if in_jobs and line and not line.startswith((" ", "\t")):
            in_jobs = False
            current = None
            continue
        if not in_jobs:
            continue
        if (
            line.startswith("  ")
            and not line.startswith("   ")
            and line.rstrip().endswith(":")
            and not line.lstrip().startswith("-")
        ):
            current = line.strip()[:-1]
            bodies[current] = [line]
        elif current is not None:
            bodies[current].append(line)
    return {job_id: "\n".join(lines) for job_id, lines in bodies.items()}


def active_code_lines(text: str) -> list[str]:
    lines: list[str] = []
    for raw in text.splitlines():
        stripped = raw.strip()
        if not stripped or stripped.startswith("#"):
            continue
        lines.append(raw)
    return lines


def validate_rust_small_fmt_contract(workflow_text: str) -> None:
    jobs = job_bodies(workflow_text)
    missing = [job_id for job_id in RUST_SMALL_LANE_JOBS if job_id not in jobs]
    if missing:
        raise AssertionError(f"required Rust Small lane jobs missing: {missing}")

    for job_id in RUST_SMALL_LANE_JOBS:
        body = jobs[job_id]
        active = "\n".join(active_code_lines(body))
        if FMT_COMMAND not in active:
            raise AssertionError(
                f"{job_id} must run workspace-wide {FMT_COMMAND!r}; "
                "commenting it out or deleting it is a silent revert of #12320"
            )
        if "git diff --name-only" in active:
            raise AssertionError(
                f"{job_id} reintroduced changed-file narrowing around rustfmt"
            )
        if CARGO_FMT_RE.search(active) and re.search(
            r"cargo\s+fmt\b(?![^\n]*--all)", active
        ):
            raise AssertionError(
                f"{job_id} must keep cargo fmt --all; dropping --all reintroduces "
                "changed-file or crate-local narrowing"
            )
        if re.search(r"cargo\s+fmt\b[^\n]*--files\b", active):
            raise AssertionError(
                f"{job_id} must not pass --files to cargo fmt / rustfmt"
            )

    result_job = jobs.get(RUST_SMALL_RESULT_JOB)
    if not isinstance(result_job, str):
        raise AssertionError("Perl LSP Rust Small Result job is missing")
    result_active = "\n".join(active_code_lines(result_job))
    if "python3 -m unittest" not in result_active:
        raise AssertionError(
            "Perl LSP Rust Small Result must invoke the rustfmt prevention tests"
        )
    for test_file in CONTRACT_TEST_FILES:
        if test_file not in result_active:
            raise AssertionError(
                f"Perl LSP Rust Small Result must run {test_file} so a silent "
                "revert fails a required check"
            )
    prove = _named_step_body(result_job, "Prove rustfmt prevention contract")
    if re.search(r"^\s+if:", prove, re.MULTILINE):
        raise AssertionError(
            "rustfmt prevention contract must not be skipped with if: "
            "(draft-skip stays in the evaluate step, #10006)"
        )
    if "continue-on-error: true" in prove:
        raise AssertionError("rustfmt prevention contract must not continue on error")
    if "router was skipped (draft PR" not in result_job:
        raise AssertionError(
            "draft-skip remains owned by #10006; do not absorb it into this contract"
        )


def _named_step_body(job_text: str, step_name: str) -> str:
    marker = f"- name: {step_name}"
    start = job_text.find(marker)
    if start < 0:
        raise AssertionError(f"missing step {step_name!r}")
    rest = job_text[start + len(marker) :]
    next_step = rest.find("\n      - name:")
    return rest if next_step < 0 else rest[:next_step]


class RustSmallRequiredFmtTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow_text = load_rust_small_workflow_text()

    def test_checked_in_rust_small_fmt_contract_holds(self) -> None:
        validate_rust_small_fmt_contract(self.workflow_text)

    def test_commenting_out_one_lane_fmt_fails_closed(self) -> None:
        broken = self.workflow_text.replace(FMT_COMMAND, f"# {FMT_COMMAND}", 1)
        with self.assertRaisesRegex(AssertionError, "silent revert of #12320"):
            validate_rust_small_fmt_contract(broken)

    def test_dropping_all_flag_fails_closed(self) -> None:
        broken = self.workflow_text.replace(FMT_COMMAND, "cargo fmt -- --check", 1)
        with self.assertRaisesRegex(AssertionError, "dropping --all|silent revert"):
            validate_rust_small_fmt_contract(broken)

    def test_changed_file_narrowing_fails_closed(self) -> None:
        broken = self.workflow_text.replace(
            FMT_COMMAND,
            "cargo fmt --all -- --check --files $(git diff --name-only origin/main)",
            1,
        )
        with self.assertRaisesRegex(AssertionError, "changed-file narrowing|--files"):
            validate_rust_small_fmt_contract(broken)

    def test_removing_result_job_test_invocation_fails_closed(self) -> None:
        broken = "\n".join(
            line
            for line in self.workflow_text.splitlines()
            if "python3 -m unittest" not in line
            and all(test_file not in line for test_file in CONTRACT_TEST_FILES)
        )
        with self.assertRaisesRegex(
            AssertionError, "must invoke the rustfmt prevention tests|must run "
        ):
            validate_rust_small_fmt_contract(broken)

    def test_skipping_the_contract_step_with_if_fails_closed(self) -> None:
        broken = self.workflow_text.replace(
            "- name: Prove rustfmt prevention contract\n        shell: bash",
            "- name: Prove rustfmt prevention contract\n        if: false\n        shell: bash",
        )
        with self.assertRaisesRegex(AssertionError, "must not be skipped with if"):
            validate_rust_small_fmt_contract(broken)


if __name__ == "__main__":
    unittest.main()

