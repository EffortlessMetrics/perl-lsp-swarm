#!/usr/bin/env python3
"""Static regression contract for the paused Droid security workflow."""

from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCAN_WORKFLOW = ROOT / ".github/workflows/droid-security-scan.yml"
CONTRACT_WORKFLOW = ROOT / ".github/workflows/droid-security-boundary.yml"
FULL_SHA_ACTION = re.compile(
    r"(?m)^\s*(?:-\s*)?uses:\s+"
    r"[0-9A-Za-z_.-]+/[0-9A-Za-z_.-]+@[0-9a-f]{40}(?:\s+#.*)?$"
)


class DroidSecurityBoundaryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.scan = SCAN_WORKFLOW.read_text(encoding="utf-8")
        cls.contract = CONTRACT_WORKFLOW.read_text(encoding="utf-8")

    def test_execution_workflow_is_manual_only(self) -> None:
        self.assertRegex(self.scan, r"(?m)^\s{2}workflow_dispatch:\s*\{\}\s*$")
        self.assertNotRegex(self.scan, r"(?m)^\s{2}schedule\s*:")
        self.assertNotRegex(self.scan, r"(?m)^\s{2}pull_request(?:_target)?\s*:")
        self.assertNotRegex(self.scan, r"(?m)^\s{2}push\s*:")

    def test_execution_workflow_has_read_only_authority(self) -> None:
        self.assertRegex(
            self.scan,
            r"(?m)^permissions:\n\s{2}contents:\s+read\s*$",
        )
        self.assertNotRegex(self.scan, r"(?m)^\s*[A-Za-z-]+:\s+write\s*$")
        self.assertNotIn("write-all", self.scan)
        self.assertNotIn("id-token:", self.scan)

    def test_no_secret_or_mutable_tooling_is_reachable(self) -> None:
        forbidden = (
            "secrets.",
            "MINIMAX_API_KEY",
            "FACTORY_API_KEY",
            "droid-action",
            "factory.ai",
            "factory-plugins",
            "bun install",
            "curl ",
            "actions/checkout",
            "uses:",
            "self-hosted",
        )
        for token in forbidden:
            with self.subTest(token=token):
                self.assertNotIn(token, self.scan)

    def test_pause_is_explicit_truthful_and_routed(self) -> None:
        required = (
            "Droid security scan is paused",
            "issues/6098",
            "no checkout",
            "provider-secret access",
            "OIDC exchange",
            "no review",
            "merge authority",
        )
        lowered = self.scan.lower()
        for token in required:
            with self.subTest(token=token):
                self.assertIn(token.lower(), lowered)

    def test_contract_workflow_is_path_scoped_and_read_only(self) -> None:
        for path in (
            ".github/workflows/droid-security-scan.yml",
            ".github/workflows/droid-security-boundary.yml",
            "scripts/ci/test_droid_security_boundary.py",
        ):
            self.assertIn(path, self.contract)
        self.assertRegex(
            self.contract,
            r"(?m)^permissions:\n\s{2}contents:\s+read\s*$",
        )
        self.assertNotRegex(
            self.contract,
            r"(?m)^\s*[A-Za-z-]+:\s+write\s*$",
        )

    def test_contract_checkout_is_immutable_and_does_not_persist_credentials(self) -> None:
        action_lines = [
            line
            for line in self.contract.splitlines()
            if line.lstrip().startswith(("uses:", "- uses:"))
        ]
        self.assertEqual(len(action_lines), 1)
        self.assertRegex(action_lines[0], FULL_SHA_ACTION)
        self.assertIn("actions/checkout@", action_lines[0])
        self.assertRegex(
            self.contract,
            r"(?m)^\s+persist-credentials:\s+false\s*$",
        )

    def test_contract_executes_only_the_focused_static_test(self) -> None:
        run_lines = [
            line.strip()
            for line in self.contract.splitlines()
            if line.strip().startswith("run:")
        ]
        self.assertEqual(
            run_lines,
            ["run: python3 -m unittest scripts/ci/test_droid_security_boundary.py"],
        )
        forbidden = ("secrets.", "id-token:", "self-hosted", "droid-action")
        for token in forbidden:
            with self.subTest(token=token):
                self.assertNotIn(token, self.contract)


if __name__ == "__main__":
    unittest.main()
