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
        self.hook_content = "#!/bin/sh\nprintf 'current fixture hook\\n'\n"
        authority = self.repo / "hooks/pre-push"
        authority.parent.mkdir()
        authority.write_text(self.hook_content)
        self.git("add", "tracked", "hooks/pre-push")
        self.git("-c", "core.hooksPath=/dev/null", "commit", "-qm", "fixture")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        self.default_hook = self.install_hook(self.repo / ".git/hooks/pre-push")
        shutil.copyfile(SOURCE, self.repo / "justfile")
        self.config = self.repo / ".git/config"

    def git(self, *args):
        return subprocess.run(
            ["git", *args], cwd=self.repo, env=self.env,
            text=True, capture_output=True, check=True,
        ).stdout.strip()

    def install_hook(self, path, content=None, mode=0o755):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(self.hook_content if content is None else content)
        path.chmod(mode)
        return path

    def run_hook(self, cwd=None):
        return subprocess.run(
            ["git", "hook", "run", "pre-push"], cwd=cwd or self.repo, env=self.env,
            text=True, capture_output=True, timeout=20,
        )

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

    def assert_hook_issue(self, result, message):
        # Ordinary findings remain advisory; only an early diagnostic failure blocks.
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(message, result.stdout)
        self.assertNotIn("\u2705 pre-push hook", result.stdout)
        self.assertNotIn("all checks passed", result.stdout)

    def test_relative_hookspath_uses_current_hook_without_default(self):
        self.git("config", "--local", "core.hooksPath", "configured hooks")
        self.install_hook(self.repo / "configured hooks/pre-push")
        self.default_hook.unlink()
        hook = self.run_hook()
        self.assertEqual(hook.returncode, 0, hook.stdout + hook.stderr)
        self.assertIn("current fixture hook", hook.stdout + hook.stderr)
        result = self.diagnose()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("pre-push hook installed and current", result.stdout)
        self.assertIn("all checks passed", result.stdout)

    def test_absolute_hookspath_ignores_stale_default(self):
        active = self.root / "external hooks $cash `echo injected`/pre-push"
        self.git("config", "--local", "core.hooksPath", str(active.parent))
        self.install_hook(active)
        self.default_hook.write_text("#!/bin/sh\nexit 23\n")
        hook = self.run_hook()
        self.assertEqual(hook.returncode, 0, hook.stdout + hook.stderr)
        self.assertIn("current fixture hook", hook.stdout + hook.stderr)
        result = self.diagnose()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("all checks passed", result.stdout)

    def test_missing_effective_hook_is_not_masked_by_current_default(self):
        self.git("config", "--local", "core.hooksPath", "configured hooks")
        hook = self.run_hook()
        self.assertNotEqual(hook.returncode, 0, hook.stdout + hook.stderr)
        self.assert_hook_issue(self.diagnose(), "pre-push hook not installed")

    def test_stale_effective_hook_is_not_masked_by_current_default(self):
        self.git("config", "--local", "core.hooksPath", "configured hooks")
        self.install_hook(
            self.repo / "configured hooks/pre-push",
            "#!/bin/sh\nprintf 'stale fixture hook\\n'\nexit 23\n",
        )
        hook = self.run_hook()
        self.assertEqual(hook.returncode, 23, hook.stdout + hook.stderr)
        self.assertIn("stale fixture hook", hook.stdout + hook.stderr)
        self.assert_hook_issue(self.diagnose(), "pre-push hook installed but stale")

    def test_linked_hookspath_uses_linked_checkout_authority(self):
        linked = self.root / "linked checkout"
        self.git("worktree", "add", "-qb", "feature", str(linked))
        shutil.copyfile(SOURCE, linked / "justfile")
        linked_content = "#!/bin/sh\nprintf 'linked fixture hook\\n'\n"
        (linked / "hooks/pre-push").write_text(linked_content)
        self.git("-C", str(linked), "add", "hooks/pre-push")
        self.git("-C", str(linked), "-c", "core.hooksPath=/dev/null", "commit", "-qm", "linked hook")
        self.git("config", "--local", "core.hooksPath", ".githooks")
        self.install_hook(self.repo / ".githooks/pre-push")
        active = self.install_hook(linked / ".githooks/pre-push", linked_content)
        hook = self.run_hook(linked)
        self.assertEqual(hook.returncode, 0, hook.stdout + hook.stderr)
        self.assertIn("linked fixture hook", hook.stdout + hook.stderr)
        result = self.diagnose(linked)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("all checks passed", result.stdout)
        # The main checkout's current bytes are stale for this linked checkout.
        active.write_text(self.hook_content)
        self.assert_hook_issue(self.diagnose(linked), "pre-push hook installed but stale")

    @unittest.skipIf(os.name == "nt", "requires POSIX executable bits")
    def test_effective_hook_must_be_executable(self):
        hook_dir = "configured hooks $(touch repair-injected)"
        self.git("config", "--local", "core.hooksPath", hook_dir)
        active = self.install_hook(self.repo / hook_dir / "pre-push", mode=0o644)
        hook = self.run_hook()
        self.assertNotEqual(hook.returncode, 0, hook.stdout + hook.stderr)
        result = self.diagnose()
        self.assert_hook_issue(result, "pre-push hook present but not executable")
        action = next(line.removeprefix("   Fix: ") for line in result.stdout.splitlines()
                      if line.startswith("   Fix: chmod "))
        repair = subprocess.run(
            ["bash", "-c", action], cwd=self.repo, env=self.env,
            text=True, capture_output=True, timeout=20,
        )
        self.assertFalse((self.repo / "repair-injected").exists(), "repair expanded the hook path")
        self.assertEqual(repair.returncode, 0, repair.stdout + repair.stderr)
        self.assertTrue(os.access(active, os.X_OK))
        self.assertEqual(self.run_hook().returncode, 0)
        self.assertIn("all checks passed", self.diagnose().stdout)

    def test_effective_hook_comparison_normalizes_line_endings(self):
        self.git("config", "--local", "core.hooksPath", "configured hooks")
        self.install_hook(
            self.repo / "configured hooks/pre-push",
            self.hook_content.replace("\n", "\r\n") + "\r\n\r\n",
        )
        self.default_hook.unlink()
        result = self.diagnose()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("all checks passed", result.stdout)

    def test_hook_path_resolution_failure_is_not_healthy_or_missing(self):
        real_git = shutil.which("git")
        fake_bin = self.root / "bin"
        fake_bin.mkdir()
        wrapper = fake_bin / "git"
        wrapper.write_text(
            '#!/usr/bin/env bash\n'
            'if [[ "$1" == "rev-parse" ]]; then\n'
            '  for arg in "$@"; do\n'
            '    if [[ "$arg" == "$DOCTOR_TEST_FAIL_REV_PARSE" ]]; then\n'
            '      exit "$DOCTOR_TEST_RESOLUTION_STATUS"\n'
            '    fi\n'
            '  done\n'
            'fi\n'
            'exec "$DOCTOR_TEST_REAL_GIT" "$@"\n'
        )
        wrapper.chmod(0o755)
        self.env["PATH"] = str(fake_bin) + os.pathsep + self.env["PATH"]
        self.env["DOCTOR_TEST_REAL_GIT"] = real_git
        for argument in ("--git-path", "--show-toplevel"):
            for status in ("7", "0"):
                with self.subTest(argument=argument, status=status):
                    self.env["DOCTOR_TEST_FAIL_REV_PARSE"] = argument
                    self.env["DOCTOR_TEST_RESOLUTION_STATUS"] = status
                    result = self.diagnose()
                    self.assert_hook_issue(result, "could not resolve the current checkout's hook paths")
                    self.assertNotIn("pre-push hook not installed", result.stdout)

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
