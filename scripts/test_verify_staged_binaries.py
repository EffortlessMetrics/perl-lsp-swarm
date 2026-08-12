#!/usr/bin/env python3

from __future__ import annotations

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

WRAPPER = Path(__file__).with_name("verify-staged-binaries.sh")


class VerifyStagedBinariesAdapterTests(unittest.TestCase):
    def test_named_arguments_are_forwarded_without_positional_ambiguity(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            capture = root / "args.json"
            fake_python = root / "python3"
            fake_python.write_text(
                "#!/bin/sh\n"
                "python_args=\"$*\"\n"
                "printf '%s' \"$python_args\" | "
                "python3 -c 'import json,sys; print(json.dumps(sys.stdin.read().split()))' "
                f"> {capture!s}\n",
                encoding="utf-8",
            )
            fake_python.chmod(0o755)
            environment = os.environ.copy()
            environment["PERL_LSP_PYTHON"] = str(fake_python)

            completed = subprocess.run(
                [
                    "bash",
                    str(WRAPPER),
                    "--server",
                    "/stage/perllsp",
                    "--dap",
                    "/stage/perl-dap",
                    "--expected-version",
                    "0.18.0",
                    "--expected-target",
                    "x86_64-unknown-linux-gnu",
                    "--expected-candidate",
                    "rc1",
                    "--receipt",
                    "/stage/identity.json",
                ],
                env=environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
                text=True,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)
            arguments = json.loads(capture.read_text(encoding="utf-8"))
            self.assertIn("--server", arguments)
            self.assertIn("/stage/perllsp", arguments)
            self.assertIn("--dap", arguments)
            self.assertIn("/stage/perl-dap", arguments)
            self.assertIn("--require-dap", arguments)
            self.assertIn("--expected-candidate", arguments)
            self.assertIn("rc1", arguments)

    def test_missing_required_option_fails_before_verifier(self) -> None:
        completed = subprocess.run(
            ["bash", str(WRAPPER), "--server", "/stage/perllsp"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            text=True,
        )
        self.assertEqual(completed.returncode, 64)
        self.assertIn("Usage:", completed.stderr)

    def test_unknown_positional_argument_is_rejected(self) -> None:
        completed = subprocess.run(
            ["bash", str(WRAPPER), "server", "version", "target", "receipt"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            text=True,
        )
        self.assertEqual(completed.returncode, 64)
        self.assertIn("unknown argument", completed.stderr)


if __name__ == "__main__":
    unittest.main()
