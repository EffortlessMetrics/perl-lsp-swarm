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


class ConstrainedAdmissionTests(unittest.TestCase):
    """The byte quantities below are arithmetic fixtures, not host measurements."""

    def setUp(self):
        import hashlib
        import json
        import socket
        import sys
        from collections import namedtuple
        self.Usage = namedtuple("Usage", "total used free")
        self.gib = 1024 ** 3
        temp = tempfile.TemporaryDirectory(prefix="cargo-budget-proof-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.slot = self.root / "slot"
        self.paths = {key: self.slot / key for key in ("target", "build", "cargo_home", "temp")}
        self.args = ["check", "-p", "small", "--locked"]
        self.env = {}
        self.document = {
            "schema_version": 1,
            "scope": {"hostname": socket.gethostname(), "platform": sys.platform,
                      "working_directory": str(Path.cwd().resolve()),
                      "worktree": str(self.root), "operation": self.args[0],
                      "cargo_argv_sha256": hashlib.sha256(json.dumps(self.args, separators=(",", ":")).encode("utf-8")).hexdigest(),
                      "resources": {key: str(value) for key, value in self.paths.items()},
                      "jobs": 2, "build_environment_sha256": hashlib.sha256(b"{}").hexdigest()},
            "reserve_bytes": 8 * self.gib,
            "expected_growth_bytes": 12 * self.gib,
            "basis": "fixture:declared arithmetic bound; not a host measurement",
        }
        self.budget = self.root / "budget.json"
        self.budget.write_text(json.dumps(self.document), encoding="utf-8")

    def write_budget(self):
        import json
        self.budget.write_text(json.dumps(self.document), encoding="utf-8")

    def invoke(self, args, free=None, disk=None, call=None):
        import contextlib
        import io
        output = io.StringIO()
        free = 29 * self.gib if free is None else free
        with patch.dict(os.environ, self.env, clear=True), \
             patch.object(safe, "resource_plan", return_value=(self.root, self.slot, self.paths)), \
             patch.object(safe.shutil, "disk_usage", side_effect=disk,
                          return_value=self.Usage(32*self.gib, 32*self.gib-free, free)), \
             patch.object(safe.subprocess, "call", side_effect=call, return_value=0) as cargo, \
             contextlib.redirect_stderr(output):
            result = safe.main(args)
        return result, output.getvalue(), cargo

    def assert_unallocated(self):
        self.assertFalse(self.slot.exists())
        self.assertFalse(any(path.exists() for path in self.paths.values()))

    def test_readonly_preflight_uses_the_execution_budget_without_allocating(self):
        import json
        result, output, cargo = self.invoke(["--preflight", "--budget-file", str(self.budget), *self.args])
        self.assertEqual(result, 0, output)
        cargo.assert_not_called()
        self.assert_unallocated()
        report = json.loads(output.split("cargo-admitted preflight: ", 1)[1])
        self.assertEqual(report["verdict"], "PASS")
        self.assertEqual(report["scope"], self.document["scope"])
        self.assertEqual(report["lease_acquired"], False)
        self.assertEqual(report["admission"]["required_free_bytes"], 20*self.gib)
        self.assertEqual(report["admission"]["basis_verified"], False)
        self.assertEqual(len(report["admission"]["destinations"]), 5)

    def test_29_gib_host_executes_only_a_fitting_declared_budget(self):
        import hashlib
        import json
        result, output, cargo = self.invoke(["--budget-file", str(self.budget), *self.args])
        self.assertEqual(result, 0, output)
        cargo.assert_called_once()
        command = cargo.call_args.args[0]
        self.assertEqual(command[-3:], self.args[-3:])
        descriptor = json.loads(output.split("cargo-admitted resources: ", 1)[1])
        admission = descriptor["admission"]
        self.assertEqual(admission["budget_sha256"], hashlib.sha256(self.budget.read_bytes()).hexdigest())
        self.assertTrue(all(row["headroom_bytes"] == 9*self.gib for row in admission["destinations"]))
        self.assertFalse((self.slot / "cargo-active").exists())

    def test_oversized_budget_refuses_before_cargo_and_names_byte_shortfall(self):
        self.document["expected_growth_bytes"] = 22*self.gib
        self.write_budget()
        result, output, cargo = self.invoke(["--budget-file", str(self.budget), *self.args])
        self.assertEqual(result, 75)
        cargo.assert_not_called()
        self.assert_unallocated()
        self.assertIn("free_bytes=" + str(29*self.gib), output)
        self.assertIn("required_free_bytes=" + str(30*self.gib), output)
        self.assertIn("shortfall_bytes=" + str(self.gib), output)

    def test_exact_boundary_passes_and_one_byte_short_refuses(self):
        options = ["--preflight", "--budget-file", str(self.budget), *self.args]
        self.assertEqual(self.invoke(options, free=20*self.gib)[0], 0)
        result, _, cargo = self.invoke(options, free=20*self.gib-1)
        self.assertEqual(result, 75)
        cargo.assert_not_called()
        self.assert_unallocated()

    def test_default_policy_still_refuses_29_gib(self):
        result, _, cargo = self.invoke(self.args)
        self.assertEqual(result, 75)
        cargo.assert_not_called()
        self.assert_unallocated()

    def test_invalid_documents_cannot_allocate(self):
        import copy
        baseline = copy.deepcopy(self.document)
        cases = [(key, value) for key in ("reserve_bytes", "expected_growth_bytes")
                 for value in (0, -1, True, 0.5, "8", None, float("inf"), float("nan"), 2**63)]
        cases += [("schema_version", value) for value in (0, 2, True, "1", None)]
        cases += [("basis", value) for value in (None, "", "   ", False, "x"*4097)]
        cases += [("scope", value) for value in (None, [], "scope")]
        cases += [("unexpected", True)]
        for key, value in cases:
            with self.subTest(key=key, value=value):
                self.document = copy.deepcopy(baseline)
                self.document[key] = value
                self.write_budget()
                result, _, cargo = self.invoke(["--budget-file", str(self.budget), *self.args])
                self.assertEqual(result, 75)
                cargo.assert_not_called()
                self.assert_unallocated()
        for key in baseline:
            with self.subTest(missing=key):
                self.document = copy.deepcopy(baseline)
                del self.document[key]
                self.write_budget()
                self.assertEqual(self.invoke(["--budget-file", str(self.budget), *self.args])[0], 75)
                self.assert_unallocated()

    def test_sum_overflow_and_each_destination_refuse_before_allocation(self):
        import copy
        good = copy.deepcopy(self.document)
        self.document["reserve_bytes"] = 2**62
        self.document["expected_growth_bytes"] = 2**62
        self.write_budget()
        result, output, cargo = self.invoke(["--budget-file", str(self.budget), *self.args])
        self.assertEqual(result, 75)
        self.assertIn("reserve plus expected growth exceeds", output)
        cargo.assert_not_called()
        self.assert_unallocated()
        self.document = good
        volumes = [self.root / ("volume-" + str(i)) for i in range(5)]
        for volume in volumes:
            volume.mkdir()
        self.slot = volumes[0] / "slot"
        self.paths = {key: volumes[i+1] / "new" for i, key in enumerate(self.paths)}
        self.document["scope"]["resources"] = {key: str(value) for key, value in self.paths.items()}
        self.write_budget()
        for denied in volumes:
            seen = []
            def disk(path):
                seen.append(path)
                free = 20*self.gib-1 if path == denied else 29*self.gib
                return self.Usage(32*self.gib, 32*self.gib-free, free)
            result, _, cargo = self.invoke(["--budget-file", str(self.budget), *self.args], disk=disk)
            self.assertEqual(result, 75)
            self.assertIn(denied, seen)
            cargo.assert_not_called()
            self.assert_unallocated()

    def test_duplicate_keys_oversize_and_non_json_refuse(self):
        import json
        good = json.dumps(self.document)
        for raw in (good[:-1] + ', "reserve_bytes": 1}', '{"scope":{"hostname":"a","hostname":"b"}}',
                    " "*65537, "["*1000 + "]"*1000, "not json", "null", "[]"):
            with self.subTest(raw=raw[:60]):
                self.budget.write_text(raw, encoding="utf-8")
                result, _, cargo = self.invoke(["--budget-file", str(self.budget), *self.args])
                self.assertEqual(result, 75)
                cargo.assert_not_called()
                self.assert_unallocated()

    def test_scope_and_capacity_environment_are_not_silently_retargeted(self):
        import copy
        baseline = copy.deepcopy(self.document)
        for key, value in (("hostname", "other-host"), ("platform", "other-os"),
                           ("working_directory", str(self.root)), ("worktree", "other-tree"),
                           ("cargo_argv_sha256", "0"*64), ("operation", "test"), ("jobs", True),
                           ("jobs", 1), ("build_environment_sha256", "0"*64),
                           ("resources", {}), ("unexpected", "extra")):
            with self.subTest(key=key):
                self.document = copy.deepcopy(baseline)
                self.document["scope"][key] = value
                self.write_budget()
                result, _, cargo = self.invoke(["--budget-file", str(self.budget), *self.args])
                self.assertEqual(result, 75)
                cargo.assert_not_called()
                self.assert_unallocated()
        self.document = baseline
        self.write_budget()
        for key in ("MIN_FREE_GB", "MAX_USED_PCT", "CARGO_STORAGE_POLICY", "CARGO_EXPECTED_GROWTH_GB", "CARGO_STORAGE_BUDGET_EVIDENCE"):
            self.env = {key: ""}
            self.assertEqual(self.invoke(["--budget-file", str(self.budget), *self.args])[0], 75)
        for key, value in (("RUSTFLAGS", "-Cdebuginfo=2"), ("CARGO_PROFILE_DEV_DEBUG", "2"),
                           ("CARGO_BUILD_TARGET", "other-target"), ("CARGO_BUILD_JOBS", "1"),
                           ("PATH", "/different/toolchain"), ("CARGO_NET_OFFLINE", "true")):
            self.env = {key: value}
            result, _, cargo = self.invoke(["--budget-file", str(self.budget), *self.args])
            self.assertEqual(result, 75)
            cargo.assert_not_called()
        self.assert_unallocated()

    def test_budget_options_must_be_unique_complete_and_before_cargo(self):
        for prefix in (["--budget-file"], ["--budget-file", ""],
                       ["--preflight", "--preflight"],
                       ["--budget-file", str(self.budget), "--budget-file", str(self.budget)]):
            result, _, cargo = self.invoke([*prefix, *self.args])
            self.assertEqual(result, 75)
            cargo.assert_not_called()
            self.assert_unallocated()
        for command in (["clippy"], ["xtask"], ["check", "--config=x"], ["+stable", "check"]):
            result, _, cargo = self.invoke(["--preflight", "--budget-file", str(self.budget), *command])
            self.assertEqual(result, 75)
            cargo.assert_not_called()
            self.assert_unallocated()

    def test_preflight_reports_an_existing_lease_without_touching_it(self):
        lock = self.slot / "cargo-active"
        lock.mkdir(parents=True)
        evidence = lock / "retain"
        evidence.write_bytes(b"other-owner")
        before = evidence.stat()
        result, output, cargo = self.invoke(["--preflight", "--budget-file", str(self.budget), *self.args])
        self.assertEqual(result, 75)
        self.assertIn("slot is active", output)
        cargo.assert_not_called()
        self.assertEqual(evidence.read_bytes(), b"other-owner")
        self.assertEqual(evidence.stat().st_mtime_ns, before.st_mtime_ns)
        self.assertFalse(self.paths["target"].exists())

    def test_capacity_is_rechecked_after_lease_before_build_allocation(self):
        observations = []
        def disk(path):
            acquired = (self.slot / "cargo-active").exists()
            observations.append(acquired)
            free = self.gib if acquired else 29*self.gib
            return self.Usage(32*self.gib, 32*self.gib-free, free)
        result, _, cargo = self.invoke(["--budget-file", str(self.budget), *self.args], disk=disk)
        self.assertEqual(result, 75)
        self.assertIn(True, observations)
        cargo.assert_not_called()
        self.assertFalse((self.slot / "cargo-active").exists())
        self.assertFalse(any(path.exists() for path in self.paths.values()))

    def test_bad_capacity_observations_fail_closed(self):
        for usage in (self.Usage(0, 0, 0), self.Usage(100, 0, -1),
                      self.Usage(100, 0, 101), self.Usage(100, 0, float("nan"))):
            with self.subTest(usage=usage):
                result, _, cargo = self.invoke(["--preflight", "--budget-file", str(self.budget), *self.args],
                                                disk=lambda path, value=usage: value)
                self.assertEqual(result, 75)
                cargo.assert_not_called()
                self.assert_unallocated()
        def unavailable(path):
            raise OSError("fixture capacity unavailable")
        result, output, cargo = self.invoke(["--preflight", "--budget-file", str(self.budget), *self.args], disk=unavailable)
        self.assertEqual(result, 75)
        self.assertIn("unavailable", output)
        cargo.assert_not_called()
        self.assert_unallocated()

    def test_unlaunched_failure_releases_own_lease_but_spawn_failure_retains(self):
        original = Path.mkdir
        def fail_target(path, *args, **kwargs):
            if path == self.paths["target"]:
                raise OSError("fixture allocation failed")
            return original(path, *args, **kwargs)
        with patch.object(Path, "mkdir", fail_target):
            result, _, cargo = self.invoke(["--budget-file", str(self.budget), *self.args])
        self.assertEqual(result, 75)
        cargo.assert_not_called()
        self.assertFalse((self.slot / "cargo-active").exists())
        result, _, cargo = self.invoke(["--budget-file", str(self.budget), *self.args], call=OSError("fixture spawn failure"))
        self.assertEqual(result, 75)
        cargo.assert_called_once()
        self.assertTrue((self.slot / "cargo-active").exists())

    def test_nonregular_or_linked_budget_file_is_not_read(self):
        target = self.root / "directory"
        target.mkdir()
        self.assertEqual(self.invoke(["--budget-file", str(target), *self.args])[0], 75)
        if os.name != "nt":
            fifo = self.root / "fifo"
            os.mkfifo(fifo)
            self.assertEqual(self.invoke(["--budget-file", str(fifo), *self.args])[0], 75)
            link = self.root / "link"
            link.symlink_to(self.budget)
            self.assertEqual(self.invoke(["--budget-file", str(link), *self.args])[0], 75)
        self.assert_unallocated()


class AdmissionProcessTests(unittest.TestCase):
    """Real Git/Python processes; fake Cargo is an invocation oracle, not Rust proof."""

    def test_preflight_then_execution_bind_one_real_git_request_without_arg_leak(self):
        import json
        import sys
        import textwrap
        with tempfile.TemporaryDirectory(prefix="cargo-admission-process-") as temporary:
            root = Path(temporary)
            repo, plane = root / "repo", root / "plane"
            bin_dir = root / "bin"
            bin_dir.mkdir()
            log = root / "cargo-call.json"
            # The exact PATH is in the scope before either invocation.
            env = {key: value for key, value in os.environ.items()
                   if not key.startswith(("GIT_", "CARGO_", "RUST"))}
            env.update(DEVPLANE=str(plane), CARGO_HOME=str(root / "cargo-home"),
                       TMPDIR=str(root / "scratch"), GIT_CONFIG_NOSYSTEM="1",
                       GIT_CONFIG_GLOBAL=str(root / "no-global"),
                       PATH=str(bin_dir) + os.pathsep + os.environ.get("PATH", ""))
            subprocess.run(["git", "init", "-q", str(repo)], env=env, check=True, capture_output=True)
            fake = bin_dir / ("cargo.cmd" if os.name == "nt" else "cargo")
            # Native Windows Cargo launching is outside this POSIX process fixture.
            if os.name == "nt":
                self.skipTest("POSIX executable fixture; Windows retains existing native path tests")
            fake.write_text("#!" + sys.executable + "\n" + textwrap.dedent("""\
                import json, os, sys
                from pathlib import Path
                Path(os.environ['INVOCATION_LOG']).write_text(json.dumps({
                    'args': sys.argv[1:], 'target': os.environ['CARGO_TARGET_DIR'],
                    'build': os.environ['CARGO_BUILD_BUILD_DIR'],
                    'auto_install': os.environ['RUSTUP_AUTO_INSTALL'],
                }))
                """))
            fake.chmod(0o700)
            env["INVOCATION_LOG"] = str(log)
            args = ["run", "-p", "fixture", "--", "--credential=PRIVATE-FIXTURE-TOKEN"]
            command = [sys.executable, str(ROOT / "scripts/cargo_admitted.py")]
            def invoke(extra):
                return subprocess.run([*command, *extra, *args], cwd=repo, env=env,
                                      text=True, capture_output=True, timeout=10)
            observation = invoke(["--preflight"])
            self.assertIn(observation.returncode, (0, 75), observation.stderr)
            prefix = "cargo-admitted preflight: "
            lines = [line[len(prefix):] for line in observation.stderr.splitlines() if line.startswith(prefix)]
            self.assertEqual(len(lines), 1, observation.stderr)
            scope = json.loads(lines[0])["scope"]
            self.assertEqual(scope["worktree"], str(repo.resolve()))
            self.assertEqual(scope["working_directory"], str(repo.resolve()))
            self.assertEqual(scope["operation"], "run")
            self.assertNotIn("PRIVATE-FIXTURE-TOKEN", observation.stderr)
            self.assertFalse(plane.exists())
            self.assertFalse(log.exists())
            # One-byte budgets test the protocol without running a real build.
            policy = root / "policy.json"
            policy.write_text(json.dumps({"schema_version": 1, "scope": scope,
                              "reserve_bytes": 1, "expected_growth_bytes": 1,
                              "basis": "fixture:fake Cargo invocation; no real build"}))
            preview = invoke(["--preflight", "--budget-file", str(policy)])
            self.assertEqual(preview.returncode, 0, preview.stderr)
            self.assertFalse(plane.exists())
            self.assertFalse(log.exists())
            result = invoke(["--budget-file", str(policy)])
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn("PRIVATE-FIXTURE-TOKEN", result.stderr)
            actual = json.loads(log.read_text())
            self.assertEqual(actual["args"][-len(args[1:]):], args[1:])
            self.assertEqual(actual["auto_install"], "0")
            self.assertEqual(actual["target"], scope["resources"]["target"])
            self.assertEqual(actual["build"], scope["resources"]["build"])
            self.assertFalse(any(plane.rglob("cargo-active")))

    @unittest.skipIf(os.name == "nt", "POSIX dangling-link fixture")
    def test_preflight_does_not_call_a_dangling_lease_available(self):
        import contextlib
        import io
        from collections import namedtuple
        usage = namedtuple("Usage", "total used free")(100, 0, 100)
        with tempfile.TemporaryDirectory(prefix="cargo-lease-link-") as temporary:
            root = Path(temporary)
            slot = root / "slot"
            slot.mkdir()
            lease = slot / "cargo-active"
            lease.symlink_to(root / "missing-target", target_is_directory=True)
            with patch.dict(os.environ, {"MIN_FREE_GB": "0.000000001", "MAX_USED_PCT": "100"}, clear=True), \
                 patch.object(safe, "resource_plan", return_value=(root, slot, {"target": slot / "target"})), \
                 patch.object(safe.shutil, "disk_usage", return_value=usage), \
                 patch.object(safe.subprocess, "call") as cargo, contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(safe.main(["--preflight", "check"]), 75)
                cargo.assert_not_called()
            self.assertTrue(lease.is_symlink())
            self.assertFalse((slot / "target").exists())


if __name__ == "__main__":
    unittest.main()
