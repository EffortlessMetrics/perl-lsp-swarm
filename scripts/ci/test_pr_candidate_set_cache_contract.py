#!/usr/bin/env python3
"""Fail-closed shape contract for this candidate-set workflow (#13592).

This deliberately checks the workflow's small, fixed YAML layout, not general
YAML equivalence. Unknown layouts require review instead of being guessed at.
Shell block text stays exact: even a blank line can end a continued command.
"""

from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "pr-candidate-set.yml"
CACHE_ACTION = "uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6"
SAVE_IF = (
    "save-if: ${{ (github.event_name == 'schedule' || "
    "github.event_name == 'workflow_dispatch') && "
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch) }}"
)
CACHE_BLOCK = f"""      - name: Cache candidate-set validator build
        {CACHE_ACTION}  # v2.9.2
        with:
          cache-on-failure: true
          cache-all-crates: true
          shared-key: pr-candidate-set
          {SAVE_IF}

"""

# Pin the existing trigger, permission, runtime, and receipt contract alongside
# the cache. Comparisons below bind every field to its actual named step.
EXPECTED_WORKFLOW = """name: PR Candidate-Set Reconciliation

on:
  pull_request:
    branches: [main, master]
    types: [opened, closed, reopened, synchronize, edited, ready_for_review, converted_to_draft]
  schedule:
    - cron: '17 */6 * * *'
  workflow_dispatch: {}

concurrency:
  group: pr-candidate-set-${{ github.ref }}
  cancel-in-progress: false

permissions:
  contents: read
  issues: read
  pull-requests: read

jobs:
  reconcile:
    name: Validate current PR candidate sets
    runs-on: ubuntu-24.04
    timeout-minutes: 10
    steps:
      - name: Checkout
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1  # v7.0.1

      - name: Prove candidate-set cache contract
        run: python3 -m unittest scripts/ci/test_pr_candidate_set_cache_contract.py

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@6c977a6ca4077a0ceb28ffbe03f59d46e9ac8772  # stable (master)
        with:
          toolchain: 1.95.0

""" + CACHE_BLOCK + r"""      - name: Test candidate-set policy validator
        run: cargo test -p xtask --bin pr-candidate-set --locked

      - name: Validate policy against live GitHub cross-references
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          cargo run -p xtask --bin pr-candidate-set --locked -- \
            --live \
            --receipt target/receipts/pr-candidate-set.json

      - name: Upload candidate-set receipt
        if: always()
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a  # v7.0.1
        with:
          name: pr-candidate-set-receipt
          path: target/receipts/pr-candidate-set.json
          if-no-files-found: error
          retention-days: 14
"""


def contract_lines(block: list[str]) -> list[str]:
    """Ignore mapping comments/separators, preserving inline shell block text."""
    while block and not block[-1].strip():
        block = block[:-1]
    lines = []
    in_run = False
    for line in block:
        if line.strip() and len(line) - len(line.lstrip()) <= 8:
            in_run = line == "        run: |"
        if in_run or (line.strip() and not line.lstrip().startswith("#")):
            lines.append(line if in_run else line.rstrip())
    return lines


def workflow_blocks(workflow: str) -> dict[str, list[str]]:
    """Split the supported inline job into unique, ordered named step blocks."""
    blocks: dict[str, list[str]] = {"workflow": []}
    current = "workflow"
    for line in workflow.splitlines():
        if line.startswith("      - name: "):
            current = line.removeprefix("      - name: ")
            if current in blocks:
                raise AssertionError(f"duplicate workflow step: {current!r}")
            blocks[current] = []
        blocks[current].append(line)
    return {name: contract_lines(lines) for name, lines in blocks.items()}


def validate_cache_contract(workflow: str) -> None:
    expected = workflow_blocks(EXPECTED_WORKFLOW)
    actual = workflow_blocks(workflow)
    if list(actual) != list(expected):
        raise AssertionError(f"candidate-set step inventory/order drifted: {list(actual)!r}")
    for name, body in expected.items():
        if actual[name] != body:
            raise AssertionError(f"candidate-set active fields/commands drifted in {name!r}")


def load_workflow() -> str:
    return WORKFLOW_PATH.read_text(encoding="utf-8")


class CandidateSetCacheContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = load_workflow()

    def test_checked_in_workflow_matches_cache_contract(self) -> None:
        validate_cache_contract(self.workflow)

    def test_mapping_comments_and_blank_separators_are_not_fields(self) -> None:
        validate_cache_contract(
            self.workflow.replace(
                "          shared-key: pr-candidate-set\n",
                "          # A mapping comment does not change the key.\n\n"
                "          shared-key: pr-candidate-set\n",
            )
        )

    def test_contract_rejects_changed_execution_and_authority(self) -> None:
        cache_name = "      - name: Cache candidate-set validator build\n"
        test_command = "        run: cargo test -p xtask --bin pr-candidate-set --locked"
        live_flag = "            --live \\"
        changes = {
            "removed cache": (CACHE_BLOCK, ""),
            "floating action": (CACHE_ACTION, "uses: Swatinem/rust-cache@v2"),
            "branch action": (CACHE_ACTION, "uses: Swatinem/rust-cache@main"),
            "duplicate action field": (
                CACHE_ACTION, CACHE_ACTION + "\n        uses: Swatinem/rust-cache@main"
            ),
            "missing provenance": (CACHE_ACTION + "  # v2.9.2", CACHE_ACTION),
            "unconditional save": (SAVE_IF, "save-if: true"),
            "merged PR save": (
                SAVE_IF,
                "save-if: ${{ github.ref == 'refs/heads/main' || "
                "github.ref == 'refs/heads/master' }}",
            ),
            "missing save boundary": ("          " + SAVE_IF + "\n", ""),
            "commented save boundary": (SAVE_IF, "# " + SAVE_IF),
            "duplicate save boundary": (SAVE_IF, SAVE_IF + "\n          save-if: true"),
            "different key": ("shared-key: pr-candidate-set", "shared-key: other"),
            "uncached workspace crates": ("cache-all-crates: true", "cache-all-crates: false"),
            "disabled cache": (cache_name, cache_name + "        if: false\n"),
            "disabled compiler": (test_command, "        if: false\n" + test_command),
            "commented compiler": (
                test_command, "        run: echo skipped\n        # " + test_command.strip()
            ),
            "duplicate compiler field": (
                test_command, test_command + "\n        run: echo skipped"
            ),
            "commented live flag": (live_flag, "            # " + live_flag.strip()),
            "blank in shell continuation": (live_flag, "\n" + live_flag),
            "trailing space in shell continuation": (live_flag, live_flag + " "),
            "narrowed events": ("opened, closed, reopened", "opened, reopened"),
            "cancel active run": ("cancel-in-progress: false", "cancel-in-progress: true"),
            "duplicate permissions": ("  contents: read", "  contents: read\n  contents: write"),
        }
        for name, (before, after) in changes.items():
            with self.subTest(name=name):
                self.assertEqual(self.workflow.count(before), 1, f"mutation target: {name}")
                broken = self.workflow.replace(before, after, 1)
                self.assertNotEqual(broken, self.workflow, f"mutation did not execute: {name}")
                with self.assertRaises(AssertionError):
                    validate_cache_contract(broken)

    def test_contract_rejects_duplicate_and_reordered_steps(self) -> None:
        test_block = (
            "      - name: Test candidate-set policy validator\n"
            "        run: cargo test -p xtask --bin pr-candidate-set --locked\n\n"
        )
        live_name = "      - name: Validate policy against live GitHub cross-references\n"
        for block in (CACHE_BLOCK, test_block):
            self.assertEqual(self.workflow.count(block), 1, "step mutation target must exist once")
        self.assertEqual(self.workflow.count(live_name), 1)
        mutations = {
            "duplicate cache": self.workflow.replace(CACHE_BLOCK, CACHE_BLOCK * 2, 1),
            "extra cache action": self.workflow + CACHE_BLOCK.replace(
                "Cache candidate-set validator build", "Another cache"
            ).replace(CACHE_ACTION, "uses: Swatinem/rust-cache@main"),
            "duplicate compilation": self.workflow.replace(test_block, test_block * 2, 1),
            "cache after compilation": self.workflow.replace(CACHE_BLOCK, "", 1).replace(
                live_name, CACHE_BLOCK + live_name, 1
            ),
        }
        for name, broken in mutations.items():
            with self.subTest(name=name):
                self.assertNotEqual(broken, self.workflow, f"mutation did not execute: {name}")
                with self.assertRaises(AssertionError):
                    validate_cache_contract(broken)


if __name__ == "__main__":
    unittest.main()
