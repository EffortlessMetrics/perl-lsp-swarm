"""Check the real agent recipe's forwarding without launching a Cargo build."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parents[2] / "justfile"


class AgentNextestTests(unittest.TestCase):
    def invoke(self, status=0):
        with tempfile.TemporaryDirectory(prefix="agent-nextest-") as directory:
            root = Path(directory)
            shutil.copyfile(SOURCE, root / "justfile")
            (root / "scripts").mkdir()
            executor = root / "scripts/cargo-safe"
            executor.write_text(
                '#!/bin/sh\n'
                'printf "%s\\n" "$@" > "$ARGUMENT_LOG"\n'
                'exit "$EXECUTOR_STATUS"\n'
            )
            executor.chmod(0o755)
            log = root / "arguments"
            env = dict(os.environ, ARGUMENT_LOG=str(log),
                       EXECUTOR_STATUS=str(status))
            result = subprocess.run(
                ["just", "--justfile", str(root / "justfile"),
                 "agent-nextest"], cwd=root, env=env, capture_output=True,
                text=True, timeout=20,
            )
            return result, log.read_text().splitlines()

    def test_runner_and_cargo_profiles_match_without_changing_selection(self):
        result, arguments = self.invoke()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(arguments, ["nextest", "run", "--workspace",
                                    "--profile", "agent",
                                    "--cargo-profile", "agent"])

    def test_executor_failure_remains_failure(self):
        result, arguments = self.invoke(status=42)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("42", result.stderr)
        self.assertIn("--workspace", arguments)


if __name__ == "__main__":
    unittest.main()
