#!/usr/bin/env python3
"""Exercise a built production xtask's shared-child path with a Cargo tripwire.

Run after building the binary, before publishing it to shards. The positive
case runs the actual lint-policy checker. Controls keep local Cargo, auxiliary
and feature builds observable and reject a different claimed executable.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
COMMAND = "cargo xtask check-lint-policy"


class ProductionDispatchTests(unittest.TestCase):
    binary: Path

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.executable = self.root / "xtask with ' quote; $literal"
        try:
            os.link(self.binary, self.executable)
        except OSError:
            shutil.copy2(self.binary, self.executable)
        tools = self.root / "tools"
        tools.mkdir()
        cargo = tools / "cargo"
        cargo.write_text(
            f"#!{sys.executable}\n"
            "import json, os, sys\n"
            "with open(os.environ['CARGO_TRIPWIRE_LOG'], 'a') as log:\n"
            "    log.write(json.dumps(sys.argv[1:]) + '\\n')\n"
            "if sys.argv[1:] == ['--version']:\n"
            "    print('cargo tripwire: metadata only')\n"
            "else:\n"
            "    print('CARGO_TRIPWIRE: unexpected build', file=sys.stderr)\n"
            "    sys.exit(86)\n"
        )
        cargo.chmod(0o700)
        self.calls = self.root / "cargo-calls.jsonl"
        self.log = ROOT / "target/receipts/logs/shared_xtask_probe.log"
        self.log.unlink(missing_ok=True)
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update(
            CI_XTASK=str(self.executable),
            CARGO_TRIPWIRE_LOG=str(self.calls),
            PATH=str(tools) + os.pathsep + self.env["PATH"],
        )

    def invoke(self, command: str) -> tuple[subprocess.CompletedProcess[str], dict]:
        policy = self.root / "policy.json"
        policy.write_text(json.dumps({
            "schema_version": 1,
            "global": {"default_timeout_seconds": 30},
            "tiers": {},
            "gates": [{
                "name": "shared_xtask_probe", "tier": "merge_gate",
                "description": "Production shared-executable probe", "required": True,
                "command": command, "timeout_seconds": 30,
            }],
        }))
        receipt = self.root / "receipt.json"
        result = subprocess.run(
            [str(self.executable), "gates", "--gate", "shared_xtask_probe",
             "--gate-policy", str(policy), "--receipt", "--receipt-path", str(receipt)],
            cwd=ROOT, env=self.env, capture_output=True, text=True, timeout=45,
        )
        self.assertTrue(receipt.is_file(), result.stdout + result.stderr)
        document = json.loads(receipt.read_text())
        self.assertEqual(1, len(document["gates"]))
        gate = document["gates"][0]
        self.assertEqual(command, gate["command"], "receipt must retain the policy command")
        return result, gate

    def cargo_calls(self) -> list[list[str]]:
        return [json.loads(line) for line in self.calls.read_text().splitlines()]

    def test_shared_child_executes_real_checker_without_cargo_build(self) -> None:
        result, gate = self.invoke(COMMAND)
        self.assertEqual(0, result.returncode, result.stdout + result.stderr)
        self.assertEqual("pass", gate["status"])
        self.assertEqual(0, gate["exit_code"])
        self.assertEqual([["--version"]], self.cargo_calls())
        self.assertIn("Shared xtask child:", result.stdout)
        self.assertIn("Shared xtask child:", self.log.read_text())
        self.assertIn("check-lint-policy", self.log.read_text())
        self.assertIn("lint policy ok:", self.log.read_text(), "the real checker must execute")

    def test_local_invocation_keeps_cargo_freshness_check(self) -> None:
        self.env.pop("CI_XTASK")
        result, gate = self.invoke(COMMAND)
        self.assertNotEqual(0, result.returncode)
        self.assertEqual("fail", gate["status"])
        self.assertEqual(86, gate["exit_code"])
        self.assertIn(["xtask", "check-lint-policy"], self.cargo_calls())

    def test_different_claimed_executable_refuses_without_fallback(self) -> None:
        self.env["CI_XTASK"] = str(self.binary)
        result, gate = self.invoke(COMMAND)
        self.assertNotEqual(0, result.returncode)
        self.assertEqual("error", gate["status"])
        # command_started is an internal runner fact, not a serialized receipt
        # field. Inspect the real launch surfaces: no exit status, diagnostic
        # or checker output, and no Cargo call beyond receipt metadata.
        self.assertNotIn("exit_code", gate)
        self.assertIn("CI_XTASK does not identify", gate["output_summary"])
        self.assertNotIn("Shared xtask child:", result.stdout)
        self.assertEqual(b"", self.log.read_bytes())
        self.assertEqual([["--version"]], self.cargo_calls())

    def test_auxiliary_and_feature_builds_are_not_rewritten(self) -> None:
        for command in (
            "cargo run --locked -p xtask --bin compiler-concepts -- --check",
            "cargo run --locked -p xtask --features legacy -- check-lint-policy",
            "cargo run --locked -p xtask --profile agent -- check-lint-policy",
            "cargo xtask check-lint-policy && echo extra",
        ):
            with self.subTest(command=command):
                result, gate = self.invoke(command)
                self.assertNotEqual(0, result.returncode)
                self.assertEqual("fail", gate["status"])
                self.assertEqual(86, gate["exit_code"])
                self.assertNotIn("Shared xtask child:", result.stdout)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xtask", required=True, type=Path)
    args = parser.parse_args()
    ProductionDispatchTests.binary = args.xtask.resolve(strict=True)
    unittest.main(argv=[sys.argv[0]])
