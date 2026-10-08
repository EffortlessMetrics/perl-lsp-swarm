import importlib.util
import io
import json
import os
import re
import shutil
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[2]
HELPER = ROOT / "scripts/run-prepared-xtask.py"
spec = importlib.util.spec_from_file_location("prepared_xtask", HELPER)
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class ConfigurationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="xtask-runner-config-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.cwd = self.root / "repo"
        self.cwd.mkdir()
        self.env = {"CARGO_HOME": str(self.root / "home")}
        self.host = "x86_64-pc-windows-msvc"

    def check(self):
        runner.check_configuration(self.cwd, self.env, self.host)

    def test_plain_native_configuration_is_accepted(self):
        self.check()

    def test_environment_runner_is_refused(self):
        self.env["CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER"] = "custom"
        with self.assertRaisesRegex(ValueError, "existing Cargo runner"):
            self.check()

    def test_environment_cross_target_is_refused(self):
        self.env["CARGO_BUILD_TARGET"] = "x86_64-unknown-linux-gnu"
        with self.assertRaisesRegex(ValueError, "cross-target"):
            self.check()

    def test_configuration_customizations_are_refused(self):
        configs = ['[target.x86_64-pc-windows-msvc]\nrunner="custom"\n',
                   '[target.\'cfg(windows)\']\nrunner="custom"\n',
                   '[build]\ntarget="x86_64-unknown-linux-gnu"\n',
                   'include="elsewhere.toml"\n',
                   '[env]\nCARGO_BUILD_TARGET="x86_64-pc-windows-msvc"\n']
        config = self.cwd / ".cargo/config.toml"
        config.parent.mkdir()
        for text in configs:
            with self.subTest(text=text):
                config.write_text(text)
                with self.assertRaises(ValueError):
                    self.check()

    def test_ancestor_and_cargo_home_runners_are_refused(self):
        for directory in (self.root / ".cargo", self.root / "home"):
            with self.subTest(directory=directory):
                directory.mkdir()
                config = directory / "config"
                config.write_text('[target.x86_64-pc-windows-msvc]\nrunner="custom"\n')
                with self.assertRaisesRegex(ValueError, "existing Cargo runner"):
                    self.check()
                config.unlink()

    def test_changed_copy_never_executes(self):
        source = self.root / "xtask.exe"
        source.write_bytes(b"source")
        def corrupt_copy(_source, copied):
            Path(copied).write_bytes(b"corrupt")
        retained = self.root / "retained"
        retained.mkdir()
        with patch.object(runner.tempfile, "mkdtemp", return_value=str(retained)), \
             patch.object(runner.shutil, "copyfile", side_effect=corrupt_copy), \
             patch.object(runner.subprocess, "Popen") as execute:
            with self.assertRaisesRegex(ValueError, "changed during handoff"):
                runner.run_copy(source, ["gates"])
            execute.assert_not_called()

    def handoff(self):
        source = self.root / "xtask.exe"
        source.write_bytes(b"source")
        directory = self.root / "handoff"
        directory.mkdir()
        return source, directory, directory / "xtask.exe"

    def test_initial_hash_failure_removes_only_empty_handoff(self):
        source, directory, _ = self.handoff()
        failure = OSError("source unavailable")
        with patch.object(runner.tempfile, "mkdtemp", return_value=str(directory)), \
             patch.object(runner, "digest", side_effect=failure), \
             patch.object(runner.subprocess, "Popen") as execute:
            with self.assertRaises(OSError) as caught:
                runner.run_copy(source, ["gates"])
            self.assertIs(caught.exception, failure)
            execute.assert_not_called()
        self.assertFalse(directory.exists())
        self.assertEqual(source.read_bytes(), b"source")

    def test_partial_copy_failure_removes_only_unstarted_copy(self):
        source, directory, copied = self.handoff()
        failure = OSError("copy interrupted")
        def fail_copy(_source, destination):
            Path(destination).write_bytes(b"partial")
            raise failure
        with patch.object(runner.tempfile, "mkdtemp", return_value=str(directory)), \
             patch.object(runner.shutil, "copyfile", side_effect=fail_copy), \
             patch.object(runner.subprocess, "Popen") as execute:
            with self.assertRaises(OSError) as caught:
                runner.run_copy(source, ["gates"])
            self.assertIs(caught.exception, failure)
            execute.assert_not_called()
        self.assertFalse(copied.exists())
        self.assertFalse(directory.exists())
        self.assertEqual(source.read_bytes(), b"source")

    def test_spawn_failure_removes_unstarted_copy_without_fallback(self):
        source, directory, copied = self.handoff()
        failure = OSError("creation refused")
        with patch.object(runner.tempfile, "mkdtemp", return_value=str(directory)), \
             patch.object(runner.subprocess, "Popen", side_effect=failure) as execute:
            with self.assertRaises(OSError) as caught:
                runner.run_copy(source, ["gates", "argument with spaces"])
            self.assertIs(caught.exception, failure)
            execute.assert_called_once_with([str(copied), "gates", "argument with spaces"])
        self.assertFalse(directory.exists())
        self.assertEqual(source.read_bytes(), b"source")

    def test_wait_failure_retains_and_reports_copy_with_unknown_consumer(self):
        source, directory, copied = self.handoff()
        failure = OSError("wait interrupted")
        process = Mock()
        process.wait.side_effect = failure
        diagnostics = io.StringIO()
        with patch.object(runner.tempfile, "mkdtemp", return_value=str(directory)), \
             patch.object(runner.subprocess, "Popen", return_value=process) as execute, \
             patch.object(runner.sys, "stderr", diagnostics):
            with self.assertRaises(OSError) as caught:
                runner.run_copy(source, ["gates"])
            self.assertIs(caught.exception, failure)
            execute.assert_called_once_with([str(copied), "gates"])
            process.wait.assert_called_once_with()
        self.assertEqual(copied.read_bytes(), b"source")
        self.assertIn(f"wait failed; retained {directory}", diagnostics.getvalue())
        process.terminate.assert_not_called()
        process.kill.assert_not_called()

    def test_cleanup_failure_reports_retention_preserves_original_error(self):
        source, directory, copied = self.handoff()
        failure = OSError("copy failed")
        diagnostics = io.StringIO()
        with patch.object(runner.tempfile, "mkdtemp", return_value=str(directory)), \
             patch.object(runner.shutil, "copyfile", side_effect=failure), \
             patch.object(Path, "unlink", side_effect=PermissionError("mapped")), \
             patch.object(runner.subprocess, "Popen") as execute, \
             patch.object(runner.sys, "stderr", diagnostics):
            with self.assertRaises(OSError) as caught:
                runner.run_copy(source, ["gates"])
            self.assertIs(caught.exception, failure)
            execute.assert_not_called()
        self.assertTrue(directory.exists())
        self.assertIn(f"retained at {directory}: mapped", diagnostics.getvalue())

    def test_success_preserves_exit_and_cleans_only_owned_file(self):
        source, directory, copied = self.handoff()
        process = Mock()
        process.wait.return_value = 19
        with patch.object(runner.tempfile, "mkdtemp", return_value=str(directory)), \
             patch.object(runner.subprocess, "Popen", return_value=process) as execute:
            self.assertEqual(runner.run_copy(source, ["gates"]), 19)
            execute.assert_called_once_with([str(copied), "gates"])
            process.wait.assert_called_once_with()
        self.assertFalse(directory.exists())
        self.assertEqual(source.read_bytes(), b"source")

    def test_unexpected_contents_are_preserved_after_child_exit(self):
        source, directory, copied = self.handoff()
        sentinel = directory / "unexpected"
        sentinel.write_bytes(b"preserve")
        process = Mock()
        process.wait.return_value = 19
        diagnostics = io.StringIO()
        with patch.object(runner.tempfile, "mkdtemp", return_value=str(directory)), \
             patch.object(runner.subprocess, "Popen", return_value=process), \
             patch.object(runner.sys, "stderr", diagnostics):
            self.assertEqual(runner.run_copy(source, ["gates"]), 19)
        self.assertEqual(sentinel.read_bytes(), b"preserve")
        self.assertFalse(copied.exists())
        self.assertIn(f"retained at {directory}", diagnostics.getvalue())


@unittest.skipUnless(os.name != "nt", "requires a Unix shell")
class UnixRouteTests(unittest.TestCase):
    def test_ordinary_route_preserves_arguments_and_child_failure(self):
        with tempfile.TemporaryDirectory(prefix="xtask-unix-route-") as temporary:
            root = Path(temporary)
            tools = root / "tools"
            tools.mkdir()
            cargo = tools / "cargo"
            cargo.write_text("#!/usr/bin/env python3\nimport json, sys\n"
                             "if sys.argv[1:] == ['--version']:\n"
                             "    print('cargo 1.95.0 (fixture)')\n"
                             "else:\n"
                             "    print(json.dumps(sys.argv[1:]))\n"
                             "    sys.exit(19)\n")
            cargo.chmod(0o755)
            env = os.environ.copy()
            env.pop("OS", None)
            env.update(PATH=str(tools) + os.pathsep + env["PATH"],
                       DEVPLANE=str(root / "devplane"), CARGO_HOME=str(root / "home"))
            arguments = ["xtask", "gates", "argument with spaces", "--literal=unchanged"]
            result = subprocess.run(["bash", str(ROOT / "scripts/cargo-safe"), *arguments],
                                    cwd=ROOT, env=env, capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 19, result.stdout + result.stderr)
            self.assertEqual(json.loads(result.stdout.strip()), arguments)
            self.assertNotIn("prepared xtask runner", result.stderr)


@unittest.skipUnless(os.name == "nt", "requires Windows executable image locking")
class NativeCargoTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="xtask-runner-cargo-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "src").mkdir()
        # Exercise the actual repository alias on the unprepared route too.
        (self.root / ".cargo").mkdir()
        shutil.copyfile(ROOT / ".cargo/config.toml", self.root / ".cargo/config.toml")
        (self.root / "Cargo.toml").write_text('''[package]
name="xtask"
version="0.0.0"
edition="2021"
[features]
replacement=[]
broken=[]
[workspace]
''')
        (self.root / "src/main.rs").write_text(r'''
#[cfg(feature="broken")]
compile_error!("intentional nested Cargo failure");
fn main() {
    println!("compiled replacement={}", cfg!(feature="replacement"));
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(args, vec!["gates", "argument with spaces", "--literal=unchanged"]);
    assert_eq!(std::env::var("XTASK_RUNNER_SENTINEL").unwrap(), "unchanged environment");
    assert_eq!(std::env::current_dir().unwrap(), std::path::PathBuf::from(std::env::var("FIXTURE_ROOT").unwrap()));
    assert_eq!(std::env::var("CARGO_MANIFEST_DIR").unwrap(), std::env::var("FIXTURE_ROOT").unwrap());
    let feature = std::env::var("NESTED_FEATURE").unwrap();
    let output = std::process::Command::new("cargo")
        .args(["build", "--locked", "--bin", "xtask", "--features", &feature])
        .output().unwrap();
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    if !output.status.success() { std::process::exit(output.status.code().unwrap_or(1)); }
    println!("nested Cargo rebuilt the original while its runner stayed active");
}
''')
        self.env = os.environ.copy()
        self.env.update(CARGO_TARGET_DIR=str(self.root / "target"),
                        CARGO_BUILD_BUILD_DIR=str(self.root / "build"),
                        CARGO_BUILD_JOBS="2", CARGO_INCREMENTAL="0",
                        FIXTURE_ROOT=str(self.root), XTASK_RUNNER_SENTINEL="unchanged environment",
                        NESTED_FEATURE="replacement")
        self.cargo(["generate-lockfile"], check=True)

    def cargo(self, args, check=False):
        return subprocess.run(["cargo", *args], cwd=self.root, env=self.env,
                              capture_output=True, text=True, timeout=120, check=check)

    def invocation(self, prepared):
        args = ["run", "--locked", "--package", "xtask"]
        if prepared:
            args.extend(["--config", f"target.{runner.native_host()}.runner = " +
                         json.dumps([sys.executable, str(HELPER)])])
        return self.cargo([*args, "--", "gates", "argument with spaces", "--literal=unchanged"])

    def test_actual_image_lock_red_and_immutable_handoff_green(self):
        red = self.invocation(False)
        self.assertEqual(red.returncode, 101, red.stdout + red.stderr)
        self.assertIn("failed to remove file", red.stderr)
        self.assertIn("os error 5", red.stderr)
        original = self.root / "target/debug/xtask.exe"
        green = self.invocation(True)
        self.assertEqual(green.returncode, 0, green.stdout + green.stderr)
        handed = re.findall(r"prepared xtask runner sha256=([0-9a-f]{64})", green.stderr)
        self.assertEqual(len(handed), 1, green.stderr)
        self.assertIn("nested Cargo rebuilt the original", green.stdout)
        self.assertNotEqual(runner.digest(original), handed[0])

    def test_real_nested_cargo_failure_is_propagated(self):
        self.env["NESTED_FEATURE"] = "broken"
        failure = self.invocation(True)
        self.assertEqual(failure.returncode, 101, failure.stdout + failure.stderr)
        self.assertIn("intentional nested Cargo failure", failure.stderr)
        self.assertNotIn("nested Cargo rebuilt the original", failure.stdout)

    def test_real_cargo_safe_route_uses_the_verified_handoff(self):
        git_exec = Path(subprocess.check_output(["git", "--exec-path"], text=True).strip())
        bash = git_exec.parents[2] / "usr/bin/bash.exe"
        self.assertTrue(bash.is_file(), "native Git Bash is required for cargo-safe")
        self.env["DEVPLANE"] = str(self.root / "devplane")
        # Windows execution environments need not export the OS selector.
        self.env.pop("OS", None)
        # Only this dependency-free, private fixture gets a small disk floor.
        # Production cargo-safe defaults and the enclosing admission stay intact.
        self.env["MIN_FREE_GB"] = "2"
        result = subprocess.run([str(bash), str(ROOT / "scripts/cargo-safe"), "xtask", "gates",
                                 "argument with spaces", "--literal=unchanged"],
                                cwd=self.root, env=self.env, capture_output=True, text=True, timeout=120)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("prepared xtask runner sha256=", result.stderr)
        self.assertIn("nested Cargo rebuilt the original", result.stdout)


if __name__ == "__main__":
    unittest.main()
