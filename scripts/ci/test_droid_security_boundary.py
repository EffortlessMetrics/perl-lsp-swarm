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


def _code_lines(text: str) -> list[tuple[int, str]]:
    """Return non-comment YAML lines with indentation and comment text removed."""
    lines: list[tuple[int, str]] = []
    for raw in text.splitlines():
        stripped = raw.lstrip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(raw) - len(stripped)
        lines.append((indent, stripped.split(" #", 1)[0].rstrip()))
    return lines


def _top_level_mapping(text: str, parent: str) -> list[str]:
    """Return direct child keys of a top-level YAML mapping."""
    lines = _code_lines(text)
    parent_index = next(
        (index for index, (indent, line) in enumerate(lines) if indent == 0 and line == f"{parent}:"),
        None,
    )
    if parent_index is None:
        return []
    keys: list[str] = []
    for indent, line in lines[parent_index + 1 :]:
        if indent == 0:
            break
        if indent == 2:
            match = re.match(r"([A-Za-z0-9_-]+):", line)
            if match:
                keys.append(match.group(1))
    return keys


def _permission_blocks(text: str) -> list[tuple[int, str]]:
    """Return every workflow/job permissions mapping, including inline mappings."""
    blocks: list[tuple[int, str]] = []
    lines = _code_lines(text)
    for index, (indent, line) in enumerate(lines):
        if line == "permissions:" or line.startswith("permissions: "):
            values = line.partition(":")[2].strip()
            if values:
                blocks.append((indent, values))
                continue
            children: list[str] = []
            for child_indent, child in lines[index + 1 :]:
                if child_indent <= indent:
                    break
                if child_indent == indent + 2:
                    children.append(child)
            blocks.append((indent, "\n".join(children)))
    return blocks


def _permission_values(text: str) -> list[str]:
    values: list[str] = []
    for _, block in _permission_blocks(text):
        values.extend(re.findall(r"\b(?:contents|issues|pull-requests|checks|statuses|actions|id-token|attestations)\s*:\s*([A-Za-z-]+)", block))
        values.extend(re.findall(r"\b(?:read-all|write-all)\b", block))
    return values


class DroidSecurityBoundaryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.scan = SCAN_WORKFLOW.read_text(encoding="utf-8")
        cls.contract = CONTRACT_WORKFLOW.read_text(encoding="utf-8")

    def test_execution_workflow_is_manual_only(self) -> None:
        self.assertEqual(
            _top_level_mapping(self.scan, "on"),
            ["workflow_dispatch"],
        )

    def test_execution_workflow_has_read_only_authority(self) -> None:
        self.assertTrue(_permission_blocks(self.scan))
        self.assertNotIn("write", _permission_values(self.scan))
        self.assertNotIn("write-all", _permission_values(self.scan))
        self.assertNotIn("id-token", self.scan)

    def test_no_secret_or_mutable_tooling_is_reachable(self) -> None:
        executable = "\n".join(
            line
            for _, line in _code_lines(self.scan)
            if not line.startswith(("#", "##"))
        )
        self.assertNotIn("secrets.", executable)
        self.assertNotIn("MINIMAX_API_KEY", executable)
        self.assertNotIn("FACTORY_API_KEY", executable)
        self.assertNotIn("droid-action", executable)
        self.assertNotIn("factory.ai", executable)
        self.assertNotIn("factory-plugins", executable)
        self.assertNotIn("bun install", executable)
        self.assertNotIn("curl ", executable)
        self.assertNotRegex(executable, r"(?m)^\s*uses:")
        self.assertNotRegex(executable, r"(?m)^\s*runs-on:.*self-hosted")

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
        self.assertTrue(_permission_blocks(self.contract))
        self.assertNotIn("write", _permission_values(self.contract))
        self.assertNotIn("id-token", self.contract)

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
