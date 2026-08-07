#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("workflow_security_ratchet.py")
SPEC = importlib.util.spec_from_file_location("workflow_security_ratchet", MODULE_PATH)
assert SPEC and SPEC.loader
ratchet = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ratchet)


class WorkflowSecurityRatchetTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / ".github/workflows").mkdir(parents=True)
        (self.root / ".github/actions/example").mkdir(parents=True)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def write(self, relative: str, content: str | bytes) -> Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, bytes):
            path.write_bytes(content)
        else:
            path.write_text(content, encoding="utf-8")
        return path

    def rules(self) -> list[str]:
        return [finding.rule for finding in ratchet.scan(self.root)]

    def test_detects_mutable_external_action_in_composite_action(self) -> None:
        self.write(
            ".github/actions/example/action.yml",
            "name: example\nruns:\n  using: composite\n  steps:\n    - uses: actions/cache@v4\n",
        )
        self.assertIn("mutable_action_ref", self.rules())

    def test_full_sha_action_is_clean(self) -> None:
        self.write(
            ".github/actions/example/action.yml",
            "name: example\nruns:\n  using: composite\n  steps:\n    - uses: actions/cache@" + "a" * 40 + "\n",
        )
        self.assertNotIn("mutable_action_ref", self.rules())

    def test_detects_expression_embedded_in_run_source(self) -> None:
        self.write(
            ".github/workflows/injection.yml",
            "name: injection\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo '${{ github.event.inputs.value }}'\n",
        )
        self.assertIn("expression_in_run_source", self.rules())

    def test_detects_pr_write_permissions_and_secret_reference(self) -> None:
        self.write(
            ".github/workflows/pr.yml",
            "name: pr\non:\n  pull_request:\npermissions:\n  contents: write\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - env:\n          TOKEN: ${{ secrets.SPECIAL_TOKEN }}\n        run: echo safe\n",
        )
        rules = self.rules()
        self.assertIn("pr_write_permission", rules)
        self.assertIn("pr_secret_reference", rules)

    def test_detects_persisted_checkout_credentials_on_write_surface(self) -> None:
        self.write(
            ".github/workflows/write.yml",
            "name: write\non: [workflow_dispatch]\npermissions:\n  contents: write\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@" + "a" * 40 + "\n",
        )
        self.assertIn("checkout_persists_credentials_on_write_surface", self.rules())

    def test_explicitly_disabled_checkout_credentials_are_clean(self) -> None:
        self.write(
            ".github/workflows/write.yml",
            "name: write\non: [workflow_dispatch]\npermissions:\n  contents: write\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@" + "a" * 40 + "\n        with:\n          persist-credentials: false\n",
        )
        self.assertNotIn("checkout_persists_credentials_on_write_surface", self.rules())

    def test_detects_floating_cargo_install(self) -> None:
        self.write(
            ".github/workflows/install.yml",
            "name: install\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: cargo install cargo-example --locked\n",
        )
        self.assertIn("floating_cargo_install", self.rules())

    def test_accepts_immutable_cargo_install_forms(self) -> None:
        self.write(
            ".github/workflows/install.yml",
            "name: install\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: |\n          cargo install cargo-a --version 1.2.3 --locked\n          cargo install cargo-b --git https://example.invalid/repo --rev deadbeef --locked\n          cargo install --path tools/local --locked\n",
        )
        self.assertNotIn("floating_cargo_install", self.rules())

    def test_baseline_ratchets_new_existing_and_resolved_findings(self) -> None:
        workflow = self.write(
            ".github/workflows/example.yml",
            "name: example\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v7\n",
        )
        baseline_path = self.root / "baseline.json"
        args = type("Args", (), {"max_files": 512, "max_file_bytes": 2 * 1024 * 1024, "max_total_bytes": 16 * 1024 * 1024})()
        self.assertEqual(ratchet.write_baseline(self.root, baseline_path, args), 0)
        self.assertEqual(ratchet.check(self.root, baseline_path, None, args), 0)

        workflow.write_text(
            workflow.read_text(encoding="utf-8") + "      - run: cargo install cargo-new --locked\n",
            encoding="utf-8",
        )
        report = self.root / "report.json"
        self.assertEqual(ratchet.check(self.root, baseline_path, report, args), 1)
        payload = json.loads(report.read_text(encoding="utf-8"))
        self.assertEqual(payload["counts"]["new"], 1)
        self.assertGreaterEqual(payload["counts"]["existing"], 1)

        workflow.write_text(
            "name: example\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@" + "a" * 40 + "\n",
            encoding="utf-8",
        )
        self.assertEqual(ratchet.check(self.root, baseline_path, report, args), 0)
        payload = json.loads(report.read_text(encoding="utf-8"))
        self.assertGreaterEqual(payload["counts"]["resolved"], 1)

    def test_invalid_utf8_is_an_actionable_finding(self) -> None:
        self.write(".github/workflows/invalid.yml", b"name: invalid\n\xff")
        self.assertIn("malformed_utf8", self.rules())

    def test_oversized_candidate_is_an_actionable_finding(self) -> None:
        self.write(".github/workflows/large.yml", "x" * 256)
        findings = ratchet.scan(self.root, max_file_bytes=64)
        self.assertIn("candidate_file_oversized", [finding.rule for finding in findings])

    @unittest.skipIf(os.name == "nt", "symlink semantics differ on Windows")
    def test_symlink_candidate_is_rejected_without_following_it(self) -> None:
        target = self.write("outside.yml", "name: outside\n")
        link = self.root / ".github/workflows/link.yml"
        link.symlink_to(target)
        self.assertIn("unsafe_candidate_file", self.rules())

    def test_candidate_commands_are_never_executed(self) -> None:
        sentinel = self.root / "must-not-exist"
        self.write(
            ".github/workflows/fork-candidate.yml",
            f"name: candidate\non:\n  pull_request:\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: touch {sentinel}\n",
        )
        ratchet.scan(self.root)
        self.assertFalse(sentinel.exists())

    def test_output_is_deterministic(self) -> None:
        self.write(
            ".github/workflows/z.yml",
            "name: z\non: [workflow_dispatch]\njobs:\n  z:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v7\n",
        )
        self.write(
            ".github/actions/example/action.yml",
            "name: a\nruns:\n  using: composite\n  steps:\n    - uses: actions/cache@v4\n",
        )
        first = [finding.as_dict() for finding in ratchet.scan(self.root)]
        second = [finding.as_dict() for finding in ratchet.scan(self.root)]
        self.assertEqual(first, second)


if __name__ == "__main__":
    unittest.main()
