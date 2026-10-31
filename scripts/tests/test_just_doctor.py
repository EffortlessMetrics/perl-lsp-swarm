"""Exercise the ordinary diagnostic entrypoint without touching the user's Git state.

Run: python3 -m unittest discover -s scripts/tests -p test_just_doctor.py -v
Requires Git, Bash and just on PATH; no Cargo build is performed.
"""

import os
from pathlib import Path
import shutil
import shlex
import subprocess
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parents[2] / "justfile"


class JustDoctorTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="just-doctor-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "checkout with spaces $cash `echo injected`"
        self.repo.mkdir()
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        self.global_config = self.root / "global.config"
        self.global_config.write_text("[user]\n\tname = Doctor Fixture\n\temail = doctor@example.invalid\n")
        self.env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=str(self.global_config))
        self.git("init", "-q", "-b", "main")
        (self.repo / "tracked").write_text("healthy\n")
        self.git("add", "tracked")
        self.git("-c", "core.hooksPath=/dev/null", "commit", "-qm", "fixture")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        hook = self.repo / ".git/hooks/pre-push"
        hook.write_text("#!/bin/sh\nexit 0\n")
        hook.chmod(0o755)
        shutil.copyfile(SOURCE, self.repo / "justfile")
        self.config = self.repo / ".git/config"

    def git(self, *args):
        return subprocess.run(
            ["git", *args], cwd=self.repo, env=self.env,
            text=True, capture_output=True, check=True,
        ).stdout.strip()

    def diagnose(self, cwd=None):
        cwd = cwd or self.repo
        before = (self.config.read_bytes(), self.global_config.read_bytes())
        result = subprocess.run(
            ["just", "--justfile", str(cwd / "justfile"), "doctor"],
            cwd=cwd, env=self.env, text=True, capture_output=True, timeout=20,
        )
        self.assertEqual(before, (self.config.read_bytes(), self.global_config.read_bytes()))
        self.assertNotIn("Auto-fixed", result.stdout)
        return result

    def assert_blocked(self, result):
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("all checks passed", result.stdout)
        self.assertNotIn("workspace clean", result.stdout)

    def test_true_boolean_spellings_are_read_only_and_block(self):
        for value in ("true", "yes", "on", "1"):
            with self.subTest(value=value):
                self.git("config", "--local", "core.bare", value)
                result = self.diagnose()
                self.assert_blocked(result)
                self.assertIn("core.bare", result.stdout)
                self.assertIn("config --local --unset core.bare", result.stdout)

    def test_false_and_unset_allow_healthy_diagnosis(self):
        for value in ("false", "no", "0", None):
            with self.subTest(value=value):
                if value is None:
                    self.git("config", "--local", "--unset", "core.bare")
                else:
                    self.git("config", "--local", "core.bare", value)
                result = self.diagnose()
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn("all checks passed", result.stdout)

    def test_reported_explicit_action_repairs_only_when_run(self):
        self.git("config", "--local", "core.bare", "true")
        result = self.diagnose()
        self.assert_blocked(result)
        action = next(line.removeprefix("   Fix: ") for line in result.stdout.splitlines()
                      if line.startswith("   Fix: "))
        subprocess.run(["bash", "-c", action], cwd=self.repo, env=self.env, check=True)
        self.assertEqual(self.git("rev-parse", "--is-bare-repository"), "false")
        self.assertEqual(self.diagnose().returncode, 0)

    def test_linked_worktree_does_not_repair_shared_configuration(self):
        linked = self.root / "linked checkout"
        self.git("worktree", "add", "-qb", "feature", str(linked))
        shutil.copyfile(SOURCE, linked / "justfile")
        self.git("config", "--local", "core.bare", "true")
        result = self.diagnose(linked)
        self.assert_blocked(result)
        action = next(line.removeprefix("   Fix: ") for line in result.stdout.splitlines()
                      if line.startswith("   Fix: "))
        self.assertEqual(shlex.split(action)[1], f"--git-dir={self.repo / '.git'}")

    def test_invalid_boolean_fails_without_mutation(self):
        self.git("config", "--local", "core.bare", "not-a-boolean")
        self.assert_blocked(self.diagnose())

    def test_config_instrument_failure_is_not_missing_configuration(self):
        real_git = shutil.which("git")
        fake_bin = self.root / "bin"
        fake_bin.mkdir()
        wrapper = fake_bin / "git"
        wrapper.write_text(
            '#!/usr/bin/env bash\n'
            'if [[ " $* " == *" config "* ]]; then exit 7; fi\n'
            'exec "$DOCTOR_TEST_REAL_GIT" "$@"\n'
        )
        wrapper.chmod(0o755)
        self.env["PATH"] = str(fake_bin) + os.pathsep + self.env["PATH"]
        self.env["DOCTOR_TEST_REAL_GIT"] = real_git
        result = self.diagnose()
        self.assert_blocked(result)
        self.assertIn("core.bare", result.stdout)

    def test_nonrepository_fails_without_mutation(self):
        nonrepo = self.root / "nonrepo"
        nonrepo.mkdir()
        shutil.copyfile(SOURCE, nonrepo / "justfile")
        self.assert_blocked(self.diagnose(nonrepo))


if __name__ == "__main__":
    unittest.main()
