#!/usr/bin/env python3
"""Contract tests for the PR candidate-set Rust cache (#13592)."""

from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "pr-candidate-set.yml"
CACHE_ACTION = (
    "uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6"
)
SAVE_IF = (
    "save-if: ${{ github.ref == 'refs/heads/main' || "
    "github.ref == 'refs/heads/master' }}"
)
CACHE_BLOCK = f"""      - name: Cache candidate-set validator build
        {CACHE_ACTION}
        with:
          cache-on-failure: true
          cache-all-crates: true
          shared-key: pr-candidate-set
          {SAVE_IF}

"""


def validate_cache_contract(workflow: str) -> None:
    required_fragments = (
        "types: [opened, closed, reopened, synchronize, edited, ready_for_review, converted_to_draft]",
        "- cron: '17 */6 * * *'",
        "workflow_dispatch: {}",
        "cancel-in-progress: false",
        "run: cargo test -p xtask --bin pr-candidate-set --locked",
        "--live \\",
        "--receipt target/receipts/pr-candidate-set.json",
        "retention-days: 14",
        "run: python3 -m unittest scripts/ci/test_pr_candidate_set_cache_contract.py",
    )
    for fragment in required_fragments:
        if fragment not in workflow:
            raise AssertionError(f"candidate-set workflow contract missing {fragment!r}")

    if workflow.count(CACHE_ACTION) != 1:
        raise AssertionError("candidate-set cache action must appear exactly once at its pinned SHA")
    if "Swatinem/rust-cache@v" in workflow:
        raise AssertionError("candidate-set cache action must not use a floating version")

    for setting in (
        "cache-on-failure: true",
        "cache-all-crates: true",
        "shared-key: pr-candidate-set",
        SAVE_IF,
    ):
        if setting not in workflow:
            raise AssertionError(f"candidate-set cache setting missing {setting!r}")

    setup = workflow.index("      - name: Setup Rust")
    cache = workflow.index("      - name: Cache candidate-set validator build")
    test = workflow.index("      - name: Test candidate-set policy validator")
    live = workflow.index("      - name: Validate policy against live GitHub cross-references")
    if not setup < cache < test < live:
        raise AssertionError(
            "candidate-set cache must run after Rust setup and before either Cargo command"
        )


def load_workflow() -> str:
    return WORKFLOW_PATH.read_text(encoding="utf-8")


class CandidateSetCacheContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = load_workflow()

    def test_checked_in_workflow_matches_cache_contract(self) -> None:
        validate_cache_contract(self.workflow)

    def test_cache_contract_rejects_realistic_regressions(self) -> None:
        mutations = {
            "removed action": self.workflow.replace(CACHE_BLOCK, ""),
            "floating action": self.workflow.replace(
                CACHE_ACTION, "uses: Swatinem/rust-cache@v2"
            ),
            "pr writes cache": self.workflow.replace(SAVE_IF, "save-if: true"),
            "shared key drift": self.workflow.replace(
                "shared-key: pr-candidate-set", "shared-key: unrelated-gate"
            ),
            "workspace crate cache removed": self.workflow.replace(
                "          cache-all-crates: true\n", ""
            ),
            "cache moved after compile": self.workflow.replace(CACHE_BLOCK, "").replace(
                "      - name: Validate policy against live GitHub cross-references\n",
                CACHE_BLOCK
                + "      - name: Validate policy against live GitHub cross-references\n",
            ),
        }
        for name, broken in mutations.items():
            with self.subTest(name=name):
                with self.assertRaises(AssertionError):
                    validate_cache_contract(broken)


if __name__ == "__main__":
    unittest.main()
