"""Run the commands printed by devplane-init; no Cargo build or downloads.

python3 -m unittest discover -s scripts/tests -p test_cargo_admitted_devplane_guidance.py -v
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class DevplaneGuidanceTests(unittest.TestCase):
    def test_recommendation_preserves_budget_scope_and_refusal(self):
        with tempfile.TemporaryDirectory(prefix="devplane-guidance-") as temporary:
            root = Path(temporary)
            scripts = root / "scripts"
            scripts.mkdir()
            log = root / "calls.jsonl"
            env = {key: value for key, value in os.environ.items()
                   if not key.startswith("GIT_")}
            env.update(HOME=str(root / "home with spaces"),
                       DEVPLANE=str(root / "storage with spaces"),
                       GUIDANCE_LOG=str(log), GUIDANCE_EXIT="0")
            result = subprocess.run(
                ["bash", str(ROOT / "scripts/devplane-init")], cwd=root,
                env=env, text=True, capture_output=True, timeout=10,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            recommendation = result.stdout.split("Recommended:", 1)[1]
            commands = "\n".join(line.strip() for line in recommendation.splitlines()
                                 if line.startswith("  "))
            self.assertIn("--budget-file", commands)
            self.assertNotIn("cargo-safe", commands)
            self.assertNotIn("--workspace", commands)
            self.assertIn("CONSTRAINED_CARGO_BUILDS.md", recommendation)
            self.assertIn("40 GiB", recommendation)
            # The real shell executes the printed commands, including quoting.
            # Stub only the admission boundary; production budget arithmetic is
            # independently covered by test_cargo_admitted_budget.py.
            admitted = scripts / "cargo-admitted"
            admitted.write_text(
                "#!/usr/bin/env python3\nimport json, os, sys\n"
                "with open(os.environ['GUIDANCE_LOG'], 'a') as log:\n"
                "    log.write(json.dumps(sys.argv[1:]) + '\\n')\n"
                "sys.exit(int(os.environ['GUIDANCE_EXIT']))\n",
                encoding="utf-8",
            )
            admitted.chmod(0o755)
            run = subprocess.run(["bash", "-ec", commands], cwd=root, env=env,
                                 text=True, capture_output=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stderr)
            calls = [json.loads(line) for line in log.read_text().splitlines()]
            policy = str(Path(env["HOME"]) / ".config/perl-lsp/cargo-budget.json")
            expected = ["--budget-file", policy, "check", "-p", "perl-test-must",
                        "--lib", "--locked", "--offline"]
            self.assertEqual(calls, [["--preflight", *expected], expected])
            log.unlink()
            env["GUIDANCE_EXIT"] = "75"
            run = subprocess.run(["bash", "-ec", commands], cwd=root, env=env,
                                 text=True, capture_output=True, timeout=10)
            self.assertEqual(run.returncode, 75)
            self.assertEqual(len(log.read_text().splitlines()), 1)


if __name__ == "__main__":
    unittest.main()
