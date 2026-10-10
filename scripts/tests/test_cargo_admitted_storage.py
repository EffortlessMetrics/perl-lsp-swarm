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


def git_output(path):
    return os.fsencode(path) + b"\n"


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
                     ["clean"], ["clippy"], ["build", "--out-dir=x"], ["build", "--artifact-dir=x"],
                     ["build", "--build-dir=x"], ["test", "--manifest-path=x"], ["build", "--target-dir=x"],
                     ["build", "--config=x"], ["build", "-j8"], ["check", "-Zfoo"]]:
            self.assertEqual(self.run_safe(args), 75, args)
            self.assertFalse(self.slot.exists())

    def test_joined_short_tokens_refuse_before_allocation(self):
        for options in (["-vj8"], ["-qj8"], ["-vj", "8"], ["-v", "-j8"],
                        ["-vZunstable-options"], ["-vZ", "unstable-options"],
                        ["-vC/tmp"], ["-vC", "/tmp"], ["-pfoo"], ["-Ffoo"]):
            self.assertEqual(self.run_safe(["build", *options]), 75, options)
            self.assertFalse(self.slot.exists())

    def test_separate_short_values_and_program_arguments_remain_supported(self):
        seen = []
        def cargo(command, env):
            seen.append(command)
            return 0
        args = ["test", "-v", "-vv", "-vvv", "-p", "foo", "-F", "bar", "--", "-vj8", "--config=program-value"]
        self.assertEqual(self.run_safe(args, cargo), 0)
        self.assertEqual(seen[0][-len(args[1:]):], args[1:])

    def test_template_resource_paths_refuse_before_allocation(self):
        for component in ("cache-{workspace-path-hash}", "cache-}"):
            destination = self.root / component
            env = dict(self.env, DEVPLANE=str(destination), HOME=str(self.root), USERPROFILE=str(self.root))
            with patch.dict(os.environ, env, clear=True), \
                 patch.object(safe.subprocess, "check_output", return_value=git_output(self.root / "repo")), \
                 patch.object(safe.subprocess, "call") as cargo:
                self.assertEqual(safe.main(["check"]), 75)
                cargo.assert_not_called()
            self.assertFalse(destination.exists())
        for name in ("CARGO_HOME", "TMPDIR"):
            destination = self.root / (name + "-{workspace-root}")
            env = dict(self.env, DEVPLANE=str(self.slot), HOME=str(self.root), USERPROFILE=str(self.root), **{name: str(destination)})
            with patch.dict(os.environ, env, clear=True), \
                 patch.object(safe.subprocess, "check_output", return_value=git_output(self.root / "repo")), \
                 patch.object(safe.subprocess, "call") as cargo:
                self.assertEqual(safe.main(["check"]), 75)
                cargo.assert_not_called()
            self.assertFalse(destination.exists())
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
        self.env["CARGO_UNSTABLE_UNSTABLE_OPTIONS"] = "true"
        seen = []
        def cargo(command, env):
            self.assertTrue((self.slot / "cargo-active").is_dir())
            self.assertEqual(self.run_safe(["check"]), 75)
            self.assertEqual(env["TEMP"], env["TMPDIR"])
            self.assertEqual(env["TMP"], env["TMPDIR"])
            self.assertEqual(env["CARGO_INCREMENTAL"], "0")
            self.assertIn("unstable.unstable-options=false", command)
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
        with patch.object(safe.subprocess, "check_output", return_value=git_output(self.root / "repo.git")):
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
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
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
            self.assertNotEqual(descriptors[0]["resources"][name], descriptors[1]["resources"][name])
        self.assertEqual(descriptors[0]["lease"], descriptors[1]["lease"])
        self.assertEqual(descriptors[0]["resources"]["cargo_home"], descriptors[1]["resources"]["cargo_home"])

    @unittest.skipIf(os.name == "nt", "POSIX permits trailing whitespace in directory names")
    def test_real_git_whitespace_paths_keep_distinct_private_state(self):
        repo = self.root / "repo"
        # Git permits trailing space/tab in a separate common directory too.
        # Trailing newlines in gitdir files are not supported by Git itself.
        common = self.root / "common \t"
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        env.update(GIT_CONFIG_GLOBAL=str(self.root / "no-global"), GIT_CONFIG_NOSYSTEM="1",
                   DEVPLANE=str(self.root / "plane"), CARGO_HOME=str(self.root / "cargo"))
        for key in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
            env.pop(key, None)
        def git(*args, cwd=self.root):
            return subprocess.run(["git", *args], cwd=cwd, env=env, check=True, capture_output=True).stdout
        git("init", "-b", "main", "--separate-git-dir", str(common), str(repo))
        git("-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid",
            "commit", "--allow-empty", "-m", "fixture", cwd=repo)
        trees = [self.root / ("tree" + suffix) for suffix in ("", " ", "\t", "\n", "\r")]
        for index, tree in enumerate(trees):
            git("worktree", "add", "-b", "linked-" + str(index), str(tree), cwd=repo)
        plans = []
        previous = Path.cwd()
        try:
            for tree in trees:
                os.chdir(tree)
                # Check the real Git protocol, including its one terminal LF.
                self.assertEqual(git("rev-parse", "--show-toplevel", cwd=tree), os.fsencode(tree.resolve()) + b"\n")
                self.assertEqual(git("rev-parse", "--path-format=absolute", "--git-common-dir", cwd=tree), os.fsencode(common.resolve()) + b"\n")
                with patch.dict(os.environ, env, clear=True):
                    self.assertEqual(safe.git_path("--path-format=absolute", "--git-common-dir"), common.resolve())
                    plan = safe.resource_plan(env)
                self.assertEqual(plan[0], tree.resolve())
                plans.append(plan)
        finally:
            os.chdir(previous)
        self.assertEqual(len({plan[1] for plan in plans}), 1)
        for name in ("target", "build"):
            self.assertEqual(len({plan[2][name] for plan in plans}), len(trees))
        self.assertFalse(plans[0][1].exists())

    def test_git_path_requires_one_terminator_without_stripping_path_bytes(self):
        with patch.object(safe.subprocess, "check_output", return_value=os.fsencode(self.root)):
            with self.assertRaises(safe.Denied):
                safe.git_path("--show-toplevel")
        if os.name != "nt":
            # Two LF bytes are a valid trailing LF in the path plus Git's LF.
            path = self.root / "tree\n"
            with patch.object(safe.subprocess, "check_output", return_value=git_output(path)):
                self.assertEqual(safe.git_path("--show-toplevel"), path.resolve())

    def test_real_git_location_overrides_refuse_before_allocation(self):
        repo, other = self.root / "repo", self.root / "other"
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        env.update(GIT_CONFIG_GLOBAL=str(self.root / "no-global"), GIT_CONFIG_NOSYSTEM="1",
                   DEVPLANE=str(self.root / "plane"), CARGO_HOME=str(self.root / "cargo"))
        for path in (repo, other):
            subprocess.run(["git", "init", "-q", str(path)], env=env, check=True, capture_output=True)
        for name in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
            env.pop(name, None)
        selectors = {"GIT_DIR": str(other / ".git"), "GIT_WORK_TREE": str(other),
                     "GIT_COMMON_DIR": str(other / ".git")}
        previous = Path.cwd()
        try:
            os.chdir(repo)
            redirected = dict(env, **selectors)
            # Real negative control: Git's view is no longer Cargo's CWD.
            result = subprocess.check_output(["git", "rev-parse", "--show-toplevel"], env=redirected)
            if os.name == "nt":
                # Git spells native Windows separators with forward slashes.
                self.assertTrue(result.endswith(b"\n"))
                self.assertEqual(Path(os.fsdecode(result[:-1])).resolve(), other.resolve())
            else:
                self.assertEqual(result, git_output(other.resolve()))
            for selection in (selectors, *({name: selected} for name, value in selectors.items() for selected in (value, ""))):
                with patch.dict(os.environ, dict(env, **selection), clear=True), \
                     patch.object(safe, "check_capacity", return_value={"fixture": "identity-only"}) as capacity, \
                     patch.object(safe.subprocess, "call", return_value=0) as cargo:
                    self.assertEqual(safe.main(["check"]), 75)
                    capacity.assert_not_called()
                    cargo.assert_not_called()
                self.assertFalse((self.root / "plane").exists())
        finally:
            os.chdir(previous)

    @unittest.skipIf(os.name == "nt", "POSIX filename bytes may be non-UTF-8")
    def test_real_git_non_utf8_common_and_worktree_paths_are_lossless(self):
        repo = self.root / os.fsdecode(b"repo-\xff")
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        env.update(GIT_CONFIG_GLOBAL=str(self.root / "no-global"), GIT_CONFIG_NOSYSTEM="1",
                   DEVPLANE=str(self.root / "plane"), CARGO_HOME=str(self.root / "cargo"))
        for name in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR"):
            env.pop(name, None)
        subprocess.run(["git", "init", "-q", str(repo)], env=env, check=True, capture_output=True)
        previous = Path.cwd()
        try:
            os.chdir(repo)
            with patch.dict(os.environ, env, clear=True):
                first = safe.resource_plan(env)
                self.assertEqual(first, safe.resource_plan(env))
            self.assertEqual(os.fsencode(first[0]), os.fsencode(repo.resolve()))
            self.assertNotEqual(first[2]["target"], first[2]["build"])
            self.assertFalse(first[1].exists())
        finally:
            os.chdir(previous)

    def test_same_named_worktrees_and_stale_shared_overrides(self):
        env = {"DEVPLANE": str(self.root / "plane"), "CARGO_HOME": str(self.root / "cargo")}
        common = str(self.root / "repo.git")
        plans = []
        for worktree in (self.root / "one" / "same", self.root / "two" / "same"):
            with patch.object(safe.subprocess, "check_output", side_effect=[git_output(worktree), git_output(common)]):
                plans.append(safe.resource_plan(env))
        self.assertEqual(plans[0][1], plans[1][1])  # unchanged exclusion domain
        for name, variable in (("target", "CARGO_TARGET_DIR"), ("build", "CARGO_BUILD_BUILD_DIR")):
            self.assertNotEqual(plans[0][2][name], plans[1][2][name])
            with patch.object(safe.subprocess, "check_output", side_effect=[git_output(plans[0][0]), git_output(common)]):
                self.assertEqual(safe.resource_plan(dict(env, **{variable: str(plans[0][2][name])})), plans[0])
            for stale in (plans[1][2][name], plans[0][1] / name):
                with patch.object(safe.subprocess, "check_output", side_effect=[git_output(plans[0][0]), git_output(common)]):
                    with self.assertRaises(safe.Denied):
                        safe.resource_plan(dict(env, **{variable: str(stale)}))
        self.assertFalse(plans[0][1].exists())

    def test_legacy_resources_and_common_lease_are_preserved(self):
        worktree, common = str(self.root / "repo"), str(self.root / "repo.git")
        env = dict(self.env, DEVPLANE=str(self.root / "plane"), CARGO_HOME=str(self.root / "cargo"),
                   HOME=str(self.root), USERPROFILE=str(self.root))
        with patch.object(safe.subprocess, "check_output", side_effect=[git_output(worktree), git_output(common)]):
            _, slot, paths = safe.resource_plan(env)
        for name in ("target", "build"):
            legacy = slot / name
            legacy.mkdir(parents=True)
            (legacy / "evidence").write_bytes(b"retain exact legacy bytes")
        lease = slot / "cargo-active"
        lease.mkdir()
        with patch.dict(os.environ, env, clear=True), \
             patch.object(safe.subprocess, "check_output", side_effect=[git_output(worktree), git_output(common)]), \
             patch.object(safe.subprocess, "call") as cargo:
            self.assertEqual(safe.main(["check"]), 75)
            cargo.assert_not_called()
        self.assertTrue(lease.is_dir())
        for name in ("target", "build"):
            self.assertFalse(paths[name].exists())
            self.assertEqual((slot / name / "evidence").read_bytes(), b"retain exact legacy bytes")

    @unittest.skipUnless(os.environ.get("CARGO_ADMITTED_REAL_BUILD_TEST") == "1",
                         "set CARGO_ADMITTED_REAL_BUILD_TEST=1 for the offline Cargo A/B/A regression")
    def test_real_cargo_alternating_worktrees_preserve_dependency_behavior(self):
        """Exercise real Cargo; a shared intermediate directory fails this oracle.

        Disk policy is separately fixture-tested above. This test patches only its
        observation so a tiny offline build does not pretend to admit a host budget.
        It does not touch any existing repository, target, or user Cargo defaults.
        """
        import contextlib
        import io
        import json
        repo, linked = self.root / "repo", self.root / "linked"
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        for name in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
            env.pop(name, None)
        env.update(DEVPLANE=str(self.root / "plane"), TMPDIR=str(self.root / "tmp"),
                   CARGO_NET_OFFLINE="true", CARGO_INCREMENTAL="0", CARGO_BUILD_JOBS="2",
                   GIT_CONFIG_GLOBAL=str(self.root / "no-global"), GIT_CONFIG_NOSYSTEM="1")
        def run(command, cwd):
            result = subprocess.run(command, cwd=cwd, env=env, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            return result.stdout.strip()
        def write(path, text):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        run(["git", "init", "-b", "main", str(repo)], self.root)
        write(repo / "Cargo.toml", '[workspace]\nmembers=["app","core"]\nresolver="2"\n')
        write(repo / "core/Cargo.toml", '[package]\nname="identity-core"\nversion="0.1.0"\nedition="2021"\n')
        write(repo / "core/src/lib.rs", 'pub fn value() -> u32 { 1 }\n')
        write(repo / "app/Cargo.toml", '[package]\nname="identity-app"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nidentity-core={path="../core"}\n')
        write(repo / "app/src/main.rs", 'fn main() { println!("{}", identity_core::value()); }\n')
        run(["cargo", "generate-lockfile", "--offline"], repo)
        run(["git", "add", "."], repo)
        run(["git", "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid",
             "commit", "-m", "old behavior"], repo)
        run(["git", "worktree", "add", "-b", "changed", str(linked)], repo)
        write(linked / "core/src/lib.rs", 'pub fn value() -> u32 { 2 }\n')
        # Both worktrees, including the changed library, exist before the first
        # build. No source touching/backdating is needed to expose the stale hit.
        descriptors = []
        previous = Path.cwd()
        try:
            for worktree, expected in ((repo, "1"), (linked, "2"), (repo, "1")):
                os.chdir(worktree)
                output = io.StringIO()
                with patch.dict(os.environ, env, clear=True), \
                     patch.object(safe, "check_capacity", return_value={"fixture": "identity-only"}), \
                     contextlib.redirect_stderr(output):
                    self.assertEqual(safe.main(["build", "-p", "identity-app", "--offline", "--locked"]), 0)
                descriptor = json.loads(output.getvalue().split("cargo-admitted resources: ", 1)[1])
                descriptors.append(descriptor)
                executable = Path(descriptor["resources"]["target"]) / "debug" / ("identity-app.exe" if os.name == "nt" else "identity-app")
                # This one test owns the TemporaryDirectory and runs builds
                # sequentially: no next build starts before this assertion ends.
                # It proves source isolation, not concurrent/post-lease consumers.
                self.assertEqual(run([str(executable)], worktree), expected)
        finally:
            os.chdir(previous)
        for name in ("target", "build"):
            self.assertNotEqual(descriptors[0]["resources"][name], descriptors[1]["resources"][name])
            self.assertEqual(descriptors[0]["resources"][name], descriptors[2]["resources"][name])
        self.assertEqual(descriptors[0]["lease"], descriptors[1]["lease"])

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
