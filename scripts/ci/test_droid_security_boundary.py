#!/usr/bin/env python3
"""Static regression contract for the paused Droid security workflow."""

from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/droid-security-scan.yml"


class DroidSecurityBoundaryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.source = WORKFLOW.read_text(encoding="utf-8")

    def test_execution_is_manual_only(self) -> None:
        self.assertIn("workflow_dispatch:", self.source)
        self.assertNotRegex(self.source, r"(?m)^\s*schedule\s*:")
        self.assertNotRegex(self.source, r"(?m)^\s*pull_request(?:_target)?\s*:")
        self.assertNotRegex(self.source, r"(?m)^\s*push\s*:")

    def test_workflow_has_no_write_or_oidc_authority(self) -> None:
        self.assertRegex(self.source, r"(?m)^permissions:\n\s+contents:\s+read\s*$")
        self.assertNotRegex(self.source, r"(?m)^\s*[A-Za-z-]+:\s+write\s*$")
        self.assertNotIn("write-all", self.source)
        self.assertNotIn("id-token:", self.source)

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
        )
        for token in forbidden:
            with self.subTest(token=token):
                self.assertNotIn(token, self.source)

    def test_no_repository_or_self_hosted_execution_is_present(self) -> None:
        self.assertNotIn("self-hosted", self.source)
        self.assertNotRegex(self.source, r"(?m)^\s*uses\s*:")
        self.assertNotRegex(self.source, r"(?m)^\s*env\s*:")
        self.assertNotRegex(self.source, r"(?m)^\s*container\s*:")

    def test_pause_is_explicit_and_routed(self) -> None:
        self.assertIn("Droid security scan is paused", self.source)
        self.assertIn("issues/6098", self.source)
        self.assertIn("no checkout", self.source)
        self.assertIn("no checkout", self.source.lower())
        self.assertIn("OIDC", self.source)


if __name__ == "__main__":
    unittest.main()
