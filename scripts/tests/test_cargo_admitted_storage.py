import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("cargo_admitted", ROOT / "scripts/cargo_admitted.py")
safe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(safe)


class AdmissionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="cargo-admitted-proof-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.slot = self.root / "slot"
        self.paths = {"target": self.slot / "target", "build": self.slot / "build",
                      "cargo_home": self.root / "home", "temp": self.root / "temp"}
        self.env = {"MIN_FREE_GB": "0.001", "MAX_USED_PCT": "100"}

    def run_safe(self, args, call=None):
        with patch.dict(os.environ, self.env, clear=True), \
             patch.object(safe, "resource_plan", return_value=(self.root, self.slot, self.paths)), \
             patch.object(safe.subprocess, "call", side_effect=call or (lambda *a, **kw: 0)):
            return safe.main(args)

    def test_overrides_aliases_and_global_option_bypasses_refused_before_allocation(self):
        for args in [["+1.95", "build"], ["--config", "x", "build"], ["b"], ["nextest"],
                     ["clean"], ["test", "--manifest-path=x"], ["build", "--target-dir=x"],
                     ["build", "--config=x"], ["build", "-j8"], ["check", "-Zfoo"]]:
            self.assertEqual(self.run_safe(args), 75, args)
            self.assertFalse(self.slot.exists())

    def test_low_disk_on_each_effective_resource_prevents_any_allocation(self):
        from collections import namedtuple
        Usage = namedtuple("Usage", "total used free")
        # Distinct existing ancestor per effective volume; slot remains absent.
        volumes = [self.root / str(i) for i in range(5)]
        for path in volumes:
            path.mkdir()
        self.slot = volumes[0] / "slot"
        self.paths = {key: volumes[i + 1] / "new" for i, key in enumerate(self.paths)}
        for denied in volumes:
            seen = []
            def disk(path):
                seen.append(path)
                return Usage(1000000000, 1000000000, 0) if path == denied else Usage(1000000000, 0, 1000000000)
            with patch.object(safe.shutil, "disk_usage", side_effect=disk):
                self.assertEqual(self.run_safe(["build"]), 75)
            self.assertIn(denied, seen)
            self.assertFalse(self.slot.exists())
            self.assertFalse(any(path.exists() for path in self.paths.values()))

    def test_explicit_byte_budget_accepts_large_nearly_full_volume_without_double_count(self):
        from collections import namedtuple
        Usage = namedtuple("Usage", "total used free")
        gib = 1024 ** 3
        env = {"CARGO_STORAGE_POLICY": "byte-budget", "CARGO_EXPECTED_GROWTH_GB": "20",
               "CARGO_STORAGE_BUDGET_EVIDENCE": "fixture:aggregate-upper-bound"}
        # Five destinations on one 4-TiB volume, 80 GiB free (~98% used).
        with patch.object(safe.shutil, "disk_usage", return_value=Usage(4096*gib, 4016*gib, 80*gib)):
            report = safe.check_capacity([self.slot, *self.paths.values()], env)
            self.assertEqual(report["policy"], "byte-budget")
            self.assertEqual(report["reserve_bytes"], 40*gib)
            self.assertEqual(report["expected_total_growth_bytes"], 20*gib)
            self.assertEqual(report["evidence_reference"], env["CARGO_STORAGE_BUDGET_EVIDENCE"])
            self.assertEqual(len(report["destinations"]), 5)
            self.assertTrue(all(row["required_free_bytes"] == 60*gib for row in report["destinations"]))
            self.assertTrue(all(row["headroom_bytes"] == 20*gib for row in report["destinations"]))
            # No opt-in: the inherited percentage policy still refuses.
            with self.assertRaises(safe.Denied):
                safe.check_capacity([self.slot], {})

    def test_byte_budget_exact_boundary_and_overflow(self):
        from collections import namedtuple
        Usage = namedtuple("Usage", "total used free")
        gib = 1024 ** 3
        env = {"CARGO_STORAGE_POLICY": "byte-budget", "CARGO_EXPECTED_GROWTH_GB": "20",
               "CARGO_STORAGE_BUDGET_EVIDENCE": "fixture:boundary"}
        with patch.object(safe.shutil, "disk_usage", return_value=Usage(4096*gib, 4036*gib, 60*gib)):
            report = safe.check_capacity([self.slot], env)
            self.assertEqual(report["destinations"][0]["headroom_bytes"], 0)
        with patch.object(safe.shutil, "disk_usage", return_value=Usage(4096*gib, 4036*gib+1, 60*gib-1)):
            with self.assertRaises(safe.Denied):
                safe.check_capacity([self.slot], env)
        # Each byte operand is finite; their sum overflows.
        self.env = dict(env, MIN_FREE_GB="1e299", CARGO_EXPECTED_GROWTH_GB="1e299")
        self.assertEqual(self.run_safe(["build"]), 75)
        self.assertFalse(self.slot.exists())

    def test_byte_budget_is_in_emitted_resource_descriptor(self):
        import contextlib
        import io
        import json
        from collections import namedtuple
        Usage = namedtuple("Usage", "total used free")
        gib = 1024 ** 3
        self.env = {"CARGO_STORAGE_POLICY": "byte-budget", "CARGO_EXPECTED_GROWTH_GB": "20",
                    "CARGO_STORAGE_BUDGET_EVIDENCE": "fixture:evidence-reference-only"}
        output = io.StringIO()
        with patch.object(safe.shutil, "disk_usage", return_value=Usage(4096*gib, 4016*gib, 80*gib)), contextlib.redirect_stderr(output):
            self.assertEqual(self.run_safe(["check"]), 0)
        descriptor = json.loads(output.getvalue().split("cargo-admitted resources: ", 1)[1])
        self.assertEqual(descriptor["admission"]["policy"], "byte-budget")
        self.assertEqual(descriptor["admission"]["evidence_reference"], self.env["CARGO_STORAGE_BUDGET_EVIDENCE"])
        self.assertTrue(all(row["headroom_bytes"] == 20*gib for row in descriptor["admission"]["destinations"]))

    def test_byte_budget_checks_every_destination_before_allocation(self):
        from collections import namedtuple
        Usage = namedtuple("Usage", "total used free")
        gib = 1024 ** 3
        self.env = {"CARGO_STORAGE_POLICY": "byte-budget", "CARGO_EXPECTED_GROWTH_GB": "20",
                    "CARGO_STORAGE_BUDGET_EVIDENCE": "fixture:all-destination-upper-bound"}
        volumes = [self.root / str(i) for i in range(5)]
        for path in volumes:
            path.mkdir()
        self.slot = volumes[0] / "slot"
        self.paths = {key: volumes[i + 1] / "new" for i, key in enumerate(self.paths)}
        for denied in volumes:
            seen = []
            def disk(path):
                seen.append(path)
                free = 60*gib - 1 if path == denied else 60*gib
                return Usage(4096*gib, 4096*gib-free, free)
            with patch.object(safe.shutil, "disk_usage", side_effect=disk):
                self.assertEqual(self.run_safe(["build"]), 75)
            self.assertIn(denied, seen)
            self.assertFalse(self.slot.exists())
            self.assertFalse(any(path.exists() for path in self.paths.values()))

    def test_byte_budget_invalid_or_missing_inputs_refuse_before_allocation(self):
        valid = {"CARGO_STORAGE_POLICY": "byte-budget", "CARGO_EXPECTED_GROWTH_GB": "20",
                 "CARGO_STORAGE_BUDGET_EVIDENCE": "fixture:bounded-growth"}
        cases = [{"CARGO_EXPECTED_GROWTH_GB": value} for value in ("", "NaN", "inf", "-inf", "0", "-1", "no")]
        cases += [{"CARGO_STORAGE_BUDGET_EVIDENCE": value} for value in ("", "  ")]
        cases += [{"MIN_FREE_GB": value} for value in ("39.99", "NaN", "inf", "0")]
        cases += [{"CARGO_STORAGE_POLICY": "unknown"}]
        for change in cases:
            self.env = dict(valid, **change)
            self.assertEqual(self.run_safe(["build"]), 75, change)
            self.assertFalse(self.slot.exists())
        for missing in ("CARGO_EXPECTED_GROWTH_GB", "CARGO_STORAGE_BUDGET_EVIDENCE"):
            self.env = dict(valid)
            del self.env[missing]
            self.assertEqual(self.run_safe(["build"]), 75)
            self.assertFalse(self.slot.exists())

    def test_cancellation_retains_exclusive_resource_lease(self):
        def cancel(*args, **kw):
            raise KeyboardInterrupt()
        with self.assertRaises(KeyboardInterrupt):
            self.run_safe(["run"], cancel)
        self.assertTrue((self.slot / "cargo-active").exists())
        self.assertEqual(self.run_safe(["test"]), 75)

    def test_serialized_reuse_and_fixed_both_paths(self):
        seen = []
        def cargo(command, env):
            self.assertTrue((self.slot / "cargo-active").is_dir())
            self.assertEqual(self.run_safe(["check"]), 75)
            self.assertEqual(env["TEMP"], env["TMPDIR"])
            self.assertEqual(env["TMP"], env["TMPDIR"])
            self.assertEqual(env["CARGO_INCREMENTAL"], "0")
            self.assertEqual(env["RUSTUP_AUTO_INSTALL"], "0")
            self.assertIn('env.RUSTUP_AUTO_INSTALL.value="0"', command)
            self.assertIn('env.RUSTUP_AUTO_INSTALL.force=true', command)
            self.assertIn('env.RUSTUP_AUTO_INSTALL.relative=false', command)
            self.assertTrue(any(arg.startswith("build.build-dir=") for arg in command))
            self.assertEqual(command[command.index("--target-dir") + 1], str(self.paths["target"]))
            seen.append(command)
            return 0
        for _ in range(2):
            self.assertEqual(self.run_safe(["test", "--", "--config=program-arg"], cargo), 0)
            self.assertFalse((self.slot / "cargo-active").exists())
        self.assertEqual(seen[0], seen[1])

    def test_cancel_or_failed_spawn_retains_lease_without_pid_stealing(self):
        def fail(*args, **kw):
            raise OSError("spawn failed")
        self.assertEqual(self.run_safe(["build"], fail), 75)
        self.assertTrue((self.slot / "cargo-active").exists())
        self.assertEqual(self.run_safe(["build"]), 75)

    def test_completed_failure_releases_lease_but_preserves_artifacts(self):
        def fail(*args, **kw):
            (self.paths["build"] / "evidence").write_text("retain")
            return 101
        self.assertEqual(self.run_safe(["build"], fail), 101)
        self.assertEqual((self.paths["build"] / "evidence").read_text(), "retain")
        self.assertFalse((self.slot / "cargo-active").exists())

    def test_abnormal_child_exit_retains_lease(self):
        for code in (-9, 3221225786, 9):
            # Fresh fixture slot for each termination status; never clear a
            # retained production lease just to make a retry pass.
            self.slot = self.root / ("terminated-" + str(code))
            self.paths = {name: self.slot / name for name in self.paths}
            self.assertEqual(self.run_safe(["test"], lambda *a, **kw: code), code)
            self.assertTrue((self.slot / "cargo-active").exists())
            self.assertEqual(self.run_safe(["check"]), 75)

    def test_bounded_common_repository_identity_and_explicit_override_refusal(self):
        env = {"DEVPLANE": str(self.root), "CARGO_HOME": str(self.root / "cargo")}
        with patch.object(safe.subprocess, "check_output", return_value=str(self.root / "repo.git")):
            first = safe.resource_plan(env)
            self.assertEqual(first, safe.resource_plan(env))
            with self.assertRaises(safe.Denied):
                safe.resource_plan(dict(env, CARGO_TARGET_DIR=str(self.root / "arbitrary")))
        self.assertFalse(first[1].exists())

    def test_real_git_nested_main_and_linked_worktree_identity(self):
        import contextlib
        import io
        import json
        repo = self.root / "repo"
        linked = self.root / "linked"
        env = os.environ.copy()
        env.update(GIT_CONFIG_GLOBAL=str(self.root / "no-global"), GIT_CONFIG_NOSYSTEM="1")
        def git(*args, cwd=self.root):
            subprocess.run(["git", *args], cwd=cwd, env=env, check=True, capture_output=True)
        git("init", "-b", "main", str(repo))
        git("-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid",
            "commit", "--allow-empty", "-m", "fixture", cwd=repo)
        git("worktree", "add", "-b", "linked", str(linked), cwd=repo)
        env.update(self.env, DEVPLANE=str(self.root / "cache"),
                   CARGO_HOME=str(self.root / "cargo"), TMPDIR=str(self.root / "tmp"))
        for key in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
            env.pop(key, None)
        descriptors = []
        previous = Path.cwd()
        try:
            for worktree in (repo, linked):
                nested = worktree / "nested" / "directory"
                nested.mkdir(parents=True)
                os.chdir(nested)
                output = io.StringIO()
                with patch.dict(os.environ, env, clear=True), patch.object(safe.subprocess, "call", return_value=0), contextlib.redirect_stderr(output):
                    self.assertEqual(safe.main(["check"]), 0)
                descriptor = json.loads(output.getvalue().split("cargo-admitted resources: ", 1)[1])
                self.assertEqual(Path(descriptor["worktree"]), worktree.resolve())
                self.assertNotEqual(Path(descriptor["worktree"]), nested)
                descriptors.append(descriptor)
        finally:
            os.chdir(previous)
        for name in ("target", "build"):
            self.assertEqual(descriptors[0]["resources"][name], descriptors[1]["resources"][name])

    def test_nonfinite_policy_refused(self):
        for key in ("MIN_FREE_GB", "MAX_USED_PCT"):
            for value in ("NaN", "inf", "-inf", "0", "-1"):
                with self.assertRaises(safe.Denied):
                    safe.check_capacity([self.root], {key: value})

    def test_native_and_foreign_paths(self):
        self.assertEqual(safe.native_path(str(self.root)), self.root.resolve())
        with self.assertRaises(safe.Denied):
            safe.native_path("relative/path")
        if os.name == "nt":
            self.assertEqual(safe.native_path("/c/with space/cache"), Path("C:/with space/cache"))
            for value in ("/mnt/c/cache", "/tmp/cache", "//wsl$/Ubuntu/cache"):
                with self.assertRaises(safe.Denied):
                    safe.native_path(value)
        else:
            for value in ("C:/cache", "/mnt/c/cache"):
                with self.assertRaises(safe.Denied):
                    safe.native_path(value)

    def test_symlink_or_junction_rejected(self):
        link = self.root / "link"
        if os.name == "nt":
            result = subprocess.run(["cmd", "/c", "mklink", "/J", str(link), str(self.root)], capture_output=True)
            if result.returncode:
                self.skipTest("junction creation unavailable")
        else:
            link.symlink_to(self.root, target_is_directory=True)
        try:
            with self.assertRaises(safe.Denied):
                safe.native_path(str(link / "child"))
        finally:
            if os.name == "nt":
                link.rmdir()
            else:
                link.unlink()


if __name__ == "__main__":
    unittest.main()
