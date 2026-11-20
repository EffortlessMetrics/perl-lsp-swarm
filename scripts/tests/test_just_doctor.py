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
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith(("GIT_", "BASH_FUNC_")) and k not in ("BASH_ENV", "ENV")}
        self.global_config = self.root / "global.config"
        self.global_config.write_text("[user]\n\tname = Doctor Fixture\n\temail = doctor@example.invalid\n")
        self.env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=str(self.global_config))
        template = self.root / "empty-template"
        template.mkdir()
        self.git("init", "-q", "-b", "main", f"--template={template}")
        (self.repo / "tracked").write_text("healthy\n")
        self.git("add", "tracked")
        self.git("commit", "-qm", "fixture")
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        hook = self.repo / ".git/hooks/pre-push"
        hook.parent.mkdir()
        hook.write_text("#!/bin/sh\nexit 0\n")
        hook.chmod(0o755)
        shutil.copyfile(SOURCE, self.repo / "justfile")
        self.config = self.repo / ".git/config"

    def git(self, *args, cwd=None):
        return subprocess.run(
            ["git", *args], cwd=cwd or self.repo, env=self.env,
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

    def write_hook(self, path, text="#!/bin/sh\n# current authority\nexit 0\n"):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        path.chmod(0o755)
        return path

    def observe_hooks(self, *paths, cwd=None):
        def snapshot():
            return {p: (p.read_bytes(), p.stat().st_mode) if p.exists() else None
                    for p in paths}
        before = snapshot()
        result = self.diagnose(cwd)
        self.assertEqual(before, snapshot(), "diagnosis must not repair hook files")
        return result

    def test_current_effective_hooks_do_not_need_legacy_hook(self):
        authority = self.write_hook(self.repo / "hooks/pre-push")
        effective = self.write_hook(self.repo / ".githooks/pre-push")
        legacy = self.repo / ".git/hooks/pre-push"
        legacy.unlink()
        for configured in (".githooks", str(self.repo / ".githooks")):
            with self.subTest(configured=configured):
                self.git("config", "core.hooksPath", configured)
                result = self.observe_hooks(authority, effective, legacy)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn("pre-push hook installed and current", result.stdout)
                self.assertIn("all checks passed", result.stdout)

    def test_legacy_hook_cannot_hide_missing_effective_hook(self):
        self.git("config", "core.hooksPath", ".githooks")
        authority = self.write_hook(self.repo / "hooks/pre-push")
        legacy = self.write_hook(self.repo / ".git/hooks/pre-push")
        effective = self.repo / ".githooks/pre-push"
        result = self.observe_hooks(authority, legacy, effective)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("pre-push hook not installed", result.stdout)
        self.assertNotIn("all checks passed", result.stdout)

    def test_linked_worktree_uses_its_own_hook_and_authority(self):
        linked = self.root / "linked checkout with spaces"
        self.git("worktree", "add", "-qb", "hook-linked", str(linked))
        shutil.copyfile(SOURCE, linked / "justfile")
        self.git("config", "core.hooksPath", ".githooks")
        main_authority = self.write_hook(self.repo / "hooks/pre-push", "#!/bin/sh\n# main\nexit 0\n")
        main_effective = self.write_hook(self.repo / ".githooks/pre-push", main_authority.read_text())
        authority = self.write_hook(linked / "hooks/pre-push", "#!/bin/sh\n# linked\nexit 0\n")
        effective = self.write_hook(linked / ".githooks/pre-push", authority.read_text())
        result = self.observe_hooks(main_authority, main_effective, authority, effective, cwd=linked)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("pre-push hook installed and current", result.stdout)
        self.assertIn("all checks passed", result.stdout)
        effective.write_text(main_authority.read_text())
        result = self.observe_hooks(main_authority, main_effective, authority, effective, cwd=linked)
        self.assertIn("pre-push hook installed but stale", result.stdout)
        self.assertNotIn("all checks passed", result.stdout)

    @unittest.skipIf(os.name == "nt", "native Windows does not use POSIX executable bits")
    def test_nonexecutable_effective_hook_is_reported(self):
        self.git("config", "core.hooksPath", ".githooks")
        authority = self.write_hook(self.repo / "hooks/pre-push")
        effective = self.write_hook(self.repo / ".githooks/pre-push")
        effective.chmod(0o644)
        result = self.observe_hooks(authority, effective)
        self.assertIn("pre-push hook present but not executable", result.stdout)
        self.assertIn(str(effective), result.stdout)
        self.assertNotIn("all checks passed", result.stdout)

    def test_failed_hook_reads_cannot_report_current(self):
        self.git("config", "core.hooksPath", ".githooks")
        authority = self.write_hook(self.repo / "hooks/pre-push")
        effective = self.write_hook(self.repo / ".githooks/pre-push")
        real_awk = shutil.which("awk")
        self.assertIsNotNone(real_awk)
        fake_bin = self.root / "read-failure-bin"
        fake_bin.mkdir()
        reader = fake_bin / "awk"
        reader.write_text(
            '#!/usr/bin/env bash\n'
            'if [[ "${@: -1}" == "$DOCTOR_TEST_FAIL_READ_PATH" ]]; then exit 23; fi\n'
            'exec "$DOCTOR_TEST_REAL_AWK" "$@"\n'
        )
        reader.chmod(0o755)
        self.env.update(PATH=str(fake_bin) + os.pathsep + self.env["PATH"],
                        DOCTOR_TEST_REAL_AWK=real_awk)
        for failed in (authority, effective):
            with self.subTest(failed=failed):
                self.env["DOCTOR_TEST_FAIL_READ_PATH"] = str(failed)
                result = self.observe_hooks(authority, effective)
                self.assert_blocked(result)
                self.assertIn("could not read pre-push hook", result.stdout)
                self.assertNotIn("pre-push hook installed and current", result.stdout)

    def test_hook_path_probe_failure_is_not_a_hook_verdict(self):
        real_git = shutil.which("git")
        self.assertIsNotNone(real_git)
        fake_bin = self.root / "hook-probe-bin"
        fake_bin.mkdir()
        wrapper = fake_bin / "git"
        wrapper.write_text(
            '#!/usr/bin/env bash\n'
            'if [[ "${1:-}" == rev-parse && "${2:-}" == --path-format=absolute && '
            '"${3:-}" == "$DOCTOR_TEST_FAILED_PROBE" ]]; then exit 7; fi\n'
            'exec "$DOCTOR_TEST_REAL_GIT" "$@"\n'
        )
        wrapper.chmod(0o755)
        self.env.update(PATH=str(fake_bin) + os.pathsep + self.env["PATH"],
                        DOCTOR_TEST_REAL_GIT=real_git)
        for failed in ("--git-path", "--show-toplevel"):
            with self.subTest(failed=failed):
                self.env["DOCTOR_TEST_FAILED_PROBE"] = failed
                result = self.diagnose()
                self.assert_blocked(result)
                self.assertIn("could not resolve", result.stdout)
                self.assertNotIn("pre-push hook not installed", result.stdout)

    def hook_remedy(self, state):
        """Follow the real diagnostic, stopping at an inert admission boundary."""
        scripts = self.repo / "scripts"
        (scripts / "lib").mkdir(parents=True)
        for name in ("install-githooks.sh", "githooks-bootstrap.sh",
                     "lib/cargo-toolchain-guard.sh"):
            source = SOURCE.parent / "scripts" / name
            if source.exists():
                shutil.copy2(source, scripts / name)
        shutil.copy2(SOURCE.parent / "Cargo.toml", self.repo / "Cargo.toml")
        authority = self.repo / "hooks/pre-push"
        authority.parent.mkdir()
        authority.write_text("#!/bin/sh\n# current fixture authority\nexit 0\n")
        if state == "missing":
            (self.repo / ".git/hooks/pre-push").unlink()

        self.launch_log = self.root / "launch.txt"
        self.env["DOCTOR_LAUNCH_LOG"] = str(self.launch_log)
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        cargo = bin_dir / "cargo"
        cargo.write_text(
            '#!/usr/bin/env bash\n'
            'if [[ "${1:-}" == --version ]]; then echo "cargo 1.95.0 (fixture)"; exit 0; fi\n'
            'printf "%s\\n" raw-cargo "$@" > "$DOCTOR_LAUNCH_LOG"\n'
            'exit 97\n'
        )
        cargo.chmod(0o755)
        self.env["PATH"] = str(bin_dir) + os.pathsep + self.env["PATH"]
        self.admission = scripts / "cargo-admitted"
        self.admission.write_text(
            '#!/usr/bin/env bash\n'
            'printf "%s\\n" "$@" > "$DOCTOR_LAUNCH_LOG"\n'
            'exit "${DOCTOR_ADMISSION_EXIT:-0}"\n'
        )
        self.admission.chmod(0o755)

        before = {p: p.read_bytes() for p in (self.repo / ".git/hooks").iterdir()}
        result = self.diagnose()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("all checks passed", result.stdout)
        self.assertIn("pre-push hook " + ("not installed" if state == "missing"
                                        else "installed but stale"), result.stdout)
        self.assertFalse(self.launch_log.exists(), "diagnosis must not install hooks")
        self.assertEqual(before, {p: p.read_bytes()
                                  for p in (self.repo / ".git/hooks").iterdir()})
        actions = [line.removeprefix("   Fix: ") for line in result.stdout.splitlines()
                   if line.startswith("   Fix: ") and "install-githooks" in line]
        self.assertEqual(len(actions), 1, result.stdout)
        return actions[0]

    def test_missing_hook_remedy_reaches_lightweight_admission(self):
        self.check_hook_remedy("missing")

    def test_stale_hook_remedy_reaches_lightweight_admission(self):
        self.check_hook_remedy("stale")

    def check_hook_remedy(self, state):
        action = self.hook_remedy(state)
        expected = ["run", "-p", "perl-ci-hygiene", "--bin", "perl-ci-hygiene",
                    "--profile", "agent", "--locked", "--", "install-githooks"]
        for status in (0, 37, 75):
            with self.subTest(status=status):
                self.env["DOCTOR_ADMISSION_EXIT"] = str(status)
                self.launch_log.unlink(missing_ok=True)
                result = subprocess.run(["bash", "-c", action], cwd=self.repo,
                                        env=self.env, text=True, capture_output=True, timeout=10)
                self.assertTrue(self.launch_log.exists(), "each remedy must reach admission")
                self.assertEqual(self.launch_log.read_text().splitlines(), expected)
                self.assertEqual(result.returncode, status, result.stdout + result.stderr)
        self.admission.unlink()
        self.launch_log.unlink()
        result = subprocess.run(["bash", "-c", action], cwd=self.repo,
                                env=self.env, text=True, capture_output=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.launch_log.exists(), "missing admission must not fall back to Cargo")


if __name__ == "__main__":
    unittest.main()
