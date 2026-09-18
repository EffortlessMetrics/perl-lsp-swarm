#!/usr/bin/env python3

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

WRAPPER = Path(__file__).with_name("verify-staged-binaries.sh")


def bash_binary() -> str:
    """Resolve the bash that can actually run this POSIX adapter oracle.

    CreateProcess resolves the bare name `bash` against System32 before PATH,
    so on a Windows host with WSL installed the suite silently ran under
    Linux/WSL bash, where native `F:/...` paths do not exist — every case
    failed 127 while `shutil.which("bash")` pointed at the Git/MSYS bash the
    suite was written for (#15401). Prefer the PATH bash explicitly; POSIX
    hosts keep the bare name.
    """
    if sys.platform != "win32":
        return "bash"
    for entry in os.environ.get("PATH", "").split(os.pathsep):
        if "system32" in entry.lower():
            continue
        candidate = Path(entry) / "bash.exe"
        if candidate.is_file():
            return str(candidate)
    return "bash"


def bash_path(path: Path) -> str:
    """Render `path` so bash resolves it on every host.

    Windows spawns bash with native paths; backslashes inside the argument are
    escape characters to bash, so `F:\\code\\x.sh` became
    `F:codeRust2x.sh` (exit 127) before the forward-slash form was used
    (#15401). POSIX hosts are unaffected by `as_posix`.
    """
    return path.as_posix()


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
                f"> {capture.as_posix()}\n",
                encoding="utf-8",
            )
            fake_python.chmod(0o755)
            environment = os.environ.copy()
            environment["PERL_LSP_PYTHON"] = fake_python.as_posix()

            completed = subprocess.run(
                [
                    bash_binary(),
                    bash_path(WRAPPER),
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
            # Pair binding, not mere membership: swapped --server/--dap
            # forwarding fails here.
            self.assertEqual(arguments[arguments.index("--server") + 1], "/stage/perllsp")
            self.assertEqual(arguments[arguments.index("--dap") + 1], "/stage/perl-dap")
            self.assertEqual(
                arguments[arguments.index("--expected-version") + 1], "0.18.0"
            )

    def test_server_only_invocation_omits_dap_coupling(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            capture = root / "solo-args.json"
            fake_python = root / "python3"
            fake_python.write_text(
                "#!/bin/sh\n"
                "python_args=\"$*\"\n"
                "printf '%s' \"$python_args\" | "
                "python3 -c 'import json,sys; print(json.dumps(sys.stdin.read().split()))' "
                f"> {capture.as_posix()}\n",
                encoding="utf-8",
            )
            fake_python.chmod(0o755)
            environment = os.environ.copy()
            environment["PERL_LSP_PYTHON"] = fake_python.as_posix()

            completed = subprocess.run(
                [
                    bash_binary(),
                    bash_path(WRAPPER),
                    "--server",
                    "/stage/perllsp",
                    "--expected-version",
                    "0.18.0",
                    "--expected-target",
                    "x86_64-unknown-linux-gnu",
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
            self.assertNotIn("--dap", arguments)
            self.assertNotIn("--require-dap", arguments)

    def test_missing_required_option_exits_with_usage_before_verifier(self) -> None:
        completed = subprocess.run(
            [bash_binary(), bash_path(WRAPPER), "--server", "/stage/perllsp"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            text=True,
        )
        self.assertEqual(completed.returncode, 64)
        self.assertIn("Usage:", completed.stderr)

    def test_unknown_positional_argument_is_rejected(self) -> None:
        completed = subprocess.run(
            [bash_binary(), bash_path(WRAPPER), "server", "version", "target", "receipt"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            text=True,
        )
        self.assertEqual(completed.returncode, 64)
        self.assertIn("unknown argument", completed.stderr)


if __name__ == "__main__":
    unittest.main()
