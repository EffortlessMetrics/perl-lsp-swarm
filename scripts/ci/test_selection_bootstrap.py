"""Exercise bootstrap artifact identity/refusal with a bounded admitted fixture."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class SelectionBootstrap(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="selection bootstrap ")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.shell = None
        if os.name == "nt":
            # Select Git Bash explicitly; PATH may resolve bash to WSL.
            self.shell = Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "Git/bin/bash.exe"
            if not self.shell.is_file():
                self.skipTest("native Windows bootstrap fixture requires Git Bash")
        (self.root / "scripts/ci").mkdir(parents=True)
        bootstrap = ROOT / "scripts/ci/bootstrap-selection.sh"
        (self.root / "scripts/ci/bootstrap-selection.sh").write_bytes(
            bootstrap.read_bytes().replace(b"\r\n", b"\n"))
        (self.root / "scripts/ci/bootstrap-selection.sh").chmod(0o755)
        self.source = self.root / "crates/perl-ci-hygiene/src/main.rs"
        self.source.parent.mkdir(parents=True)
        self.source.touch()
        self.binary = self.root / "private artifacts/selection"
        self.binary.parent.mkdir()
        self.binary.write_bytes(b"#!/bin/sh\nexit 0\n")
        self.binary.chmod(0o755)
        self.admitted = self.root / "scripts/cargo-admitted"
        self.admitted.write_bytes(b'#!/bin/sh\nprintf "%s\\n" "$@" > request.txt\ncat artifact.json\nexit "${FIXTURE_STATUS:-0}"\n')
        self.admitted.chmod(0o755)
        self.artifact = {"reason": "compiler-artifact", "target": {
            "name": "perl-ci-hygiene", "kind": ["bin"], "src_path": str(self.source)},
            "executable": str(self.binary)}
        self.finished = {"reason": "build-finished", "success": True}

    def run_fixture(self, rows, status=0, args=()):
        (self.root / "artifact.json").write_text("".join(json.dumps(r)+"\n" for r in rows))
        env = os.environ.copy()
        env["FIXTURE_STATUS"] = str(status)
        command = [str(self.root / "scripts/ci/bootstrap-selection.sh"), *args]
        if self.shell:
            command = [str(self.shell), str(self.root / "scripts/ci/bootstrap-selection.sh").replace("\\", "/"), *args]
        return subprocess.run(command, env=env, capture_output=True, text=True, timeout=10)

    def test_success_uses_emitted_path_and_exact_request(self):
        result = self.run_fixture([self.artifact, self.finished])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, str(self.binary) + "\n")
        self.assertEqual((self.root / "request.txt").read_text().splitlines(), [
            "build", "--locked", "-p", "perl-ci-hygiene", "--bin", "perl-ci-hygiene", "--message-format=json"])

    def test_failed_admission_cannot_return_existing_artifact(self):
        result = self.run_fixture([self.artifact, self.finished], status=75)
        self.assertEqual(result.returncode, 75)
        self.assertEqual(result.stdout, "")

    def test_wrong_source_duplicate_missing_and_failed_terminal_refuse(self):
        wrong = dict(self.artifact, target=dict(self.artifact["target"], src_path=str(self.root / "other.rs")))
        for rows in ([wrong, self.finished], [self.artifact, self.artifact, self.finished],
                     [self.finished], [self.artifact],
                     [self.artifact, dict(self.finished, success=False)]):
            with self.subTest(rows=rows):
                result = self.run_fixture(rows)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")

    def test_missing_or_nonexecutable_artifact_refuses(self):
        if os.name != "nt":
            # Windows has no POSIX executable-bit refusal to exercise.
            self.binary.chmod(0o644)
            self.assertNotEqual(self.run_fixture([self.artifact, self.finished]).returncode, 0)
        self.binary.unlink()
        self.assertNotEqual(self.run_fixture([self.artifact, self.finished]).returncode, 0)

    def test_budget_option_is_forwarded_and_extra_options_refuse(self):
        result = self.run_fixture([self.artifact, self.finished], args=("--budget-file", "scoped budget.json"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.root / "request.txt").read_text().splitlines()[:2],
                         ["--budget-file", "scoped budget.json"])
        self.assertEqual(self.run_fixture([], args=("--offline",)).returncode, 2)


if __name__ == "__main__":
    unittest.main()
