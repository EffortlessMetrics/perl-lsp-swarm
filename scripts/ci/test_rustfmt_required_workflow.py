#!/usr/bin/env python3
"""Contract fixtures for the standalone required Rust formatting context."""

from __future__ import annotations

import copy
import re
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "ci.yml"
POLICY_PATH = ROOT / ".ci" / "policies" / "required-checks.toml"
JOB_ID = "rust-formatting"
CONTEXT_NAME = "Rust formatting"
SUBJECT_EXPRESSION = (
    "github.event_name == 'pull_request' && github.event.pull_request.head.sha || "
    "github.event_name == 'merge_group' && github.event.merge_group.head_sha || github.sha"
)


def load_workflow() -> str:
    return WORKFLOW_PATH.read_text(encoding="utf-8")


def load_policy() -> dict[str, object]:
    return tomllib.loads(POLICY_PATH.read_text(encoding="utf-8"))


def job_block(workflow: str) -> str:
    match = re.search(rf"(?m)^  {re.escape(JOB_ID)}:\s*$", workflow)
    if not match:
        raise AssertionError("formatter job is missing")
    following = re.search(r"(?m)^  [a-zA-Z0-9_-]+:\s*$", workflow[match.end() :])
    end = match.end() + following.start() if following else len(workflow)
    return workflow[match.start() : end]


def validate_contract(workflow: str, policy: dict[str, object]) -> None:
    trigger_section = workflow.split("jobs:", maxsplit=1)[0]
    for event in ("pull_request", "merge_group", "push"):
        if f"  {event}:" not in trigger_section:
            raise AssertionError(f"workflow must report on {event}")
    pull_request_trigger = trigger_section.split("  pull_request:", maxsplit=1)[1].split(
        "  merge_group:", maxsplit=1
    )[0]
    if "paths:" in pull_request_trigger or "paths-ignore:" in pull_request_trigger:
        raise AssertionError("required formatter context must remain terminal for docs-only PRs")

    job = job_block(workflow)
    if f"\n    name: {CONTEXT_NAME}\n" not in job:
        raise AssertionError("required formatter context name drifted")
    if "\n    continue-on-error: true\n" in job:
        raise AssertionError("formatter job must fail on formatter or instrument failure")
    if "\n    needs:" in job or "\n    if:" in job:
        raise AssertionError("formatter context must run terminally on every triggered subject")
    if f"SUBJECT_SHA: ${{{{ {SUBJECT_EXPRESSION} }}}}" not in job:
        raise AssertionError("formatter subject must bind PR, merge-group, and push commits exactly")

    if "- name: Checkout exact formatter subject" not in job or "ref: ${{ env.SUBJECT_SHA }}" not in job:
        raise AssertionError("checkout must use the exact formatter subject")

    if "- name: Install pinned formatter toolchain" not in job or not re.search(
        r"(?m)^          toolchain: 1\.95\.0\s*$\n^          components: rustfmt\s*$", job
    ):
        raise AssertionError("formatter must remain pinned to Rust 1.95.0 rustfmt")

    for required_fragment in (
        'actual_sha="$(git rev-parse HEAD^{commit})"',
        'test "$actual_sha" = "$SUBJECT_SHA"',
        'SUBJECT_TREE_SHA=$(git rev-parse HEAD^{tree})',
        "scripts/ci/rustfmt_check.py",
        '--candidate-sha "$SUBJECT_SHA"',
        '--candidate-tree-sha "$SUBJECT_TREE_SHA"',
        "rustup which --toolchain 1.95.0 rustfmt",
    ):
        if required_fragment not in job:
            raise AssertionError(f"formatter invocation missing {required_fragment!r}")
    if "|| true" in job:
        raise AssertionError("formatter failure must not be ignored")

    if (
        "- name: Upload candidate-bound rustfmt receipt" not in job
        or "if: always()" not in job
        or "if-no-files-found: error" not in job
    ):
        raise AssertionError("receipt upload must run always and fail when evidence is absent")

    entries = [item for item in policy.get("checks", []) if item.get("name") == CONTEXT_NAME]
    if len(entries) != 1:
        raise AssertionError("required-check policy must contain exactly one formatter context")
    entry = entries[0]
    if entry.get("workflow") != ".github/workflows/ci.yml":
        raise AssertionError("formatter policy must name the owning workflow")
    if entry.get("required") is not True or entry.get("enforcement") != "github-ruleset":
        raise AssertionError("formatter policy must target required GitHub ruleset enforcement")


class RustfmtRequiredWorkflowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = load_workflow()
        self.policy = load_policy()

    def test_checked_in_workflow_and_policy_are_coherent(self) -> None:
        validate_contract(self.workflow, self.policy)

    def test_missing_merge_group_subject_fails_closed(self) -> None:
        broken = self.workflow.replace(
            "${{ " + SUBJECT_EXPRESSION + " }}",
            "${{ github.sha }}",
            1,
        )
        with self.assertRaisesRegex(AssertionError, "bind PR, merge-group, and push"):
            validate_contract(broken, self.policy)

    def test_missing_receipt_cannot_report_green(self) -> None:
        broken = self.workflow.replace("if-no-files-found: error", "if-no-files-found: warn", 1)
        with self.assertRaisesRegex(AssertionError, "fail when evidence is absent"):
            validate_contract(broken, self.policy)

    def test_advisory_policy_entry_is_rejected(self) -> None:
        broken = copy.deepcopy(self.policy)
        entry = next(item for item in broken["checks"] if item.get("name") == CONTEXT_NAME)
        entry["required"] = False
        with self.assertRaisesRegex(AssertionError, "required GitHub ruleset"):
            validate_contract(self.workflow, broken)


if __name__ == "__main__":
    unittest.main()
