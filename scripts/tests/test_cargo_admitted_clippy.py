"""Finite staged Clippy admission; fixture capacity is not host admission."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("cargo_admitted", ROOT / "scripts/cargo_admitted.py")
safe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(safe)
REQUEST = ["clippy", "-p", "consumer", "--all-targets", "--profile", "agent", "--locked", "--", "-D", "warnings"]


class ClippyAdmissionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="admitted-clippy-controls-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.slot = self.root / "slot"
        self.paths = {name: self.root / name for name in ("target", "build", "cargo_home", "temp")}
        self.worktree = Path.cwd().resolve()
        self.plan = {"pin": "1.95.0", "host": "fixture", "root": str(self.root),
                     "subjects": {name: {"path": str(self.root / name)} for name in
                                  ("cargo", "rustc", "rustdoc", "cargo-clippy", "clippy-driver")}}
        self.env = {"MIN_FREE_GB": "0.001", "MAX_USED_PCT": "100"}

    def invoke(self, args=REQUEST, status=0, env=None, call=None, versions=None, config=None):
        stderr = io.StringIO()
        with patch.dict(os.environ, env or self.env, clear=True), contextlib.redirect_stderr(stderr), \
             patch.object(safe, "resource_plan", return_value=(self.worktree, self.slot, self.paths)), \
             patch.object(safe, "clippy_toolchain", return_value=self.plan), \
             patch.object(safe, "clippy_configuration", side_effect=config or (lambda *a: [])), \
             patch.object(safe, "revalidate_clippy_toolchain"), \
             patch.object(safe, "clippy_version_check", side_effect=versions or (lambda *a: {})), \
             patch.object(safe, "call_clippy", side_effect=call or (lambda *a: status)), \
             patch.object(safe.subprocess, "call") as raw:
            result = safe.main(list(args))
            raw.assert_not_called()
        return result, stderr.getvalue()

    def test_exact_driver_dispatch_controls_both_private_roots_and_forced_child_env(self):
        observed = []
        def launch(command, env, lock):
            observed.append((command, env, lock))
            self.assertTrue((lock / next(p.name for p in lock.iterdir())).is_dir())
            return 0
        result, output = self.invoke(call=launch)
        self.assertEqual(result, 0)
        command, env, lock = observed[0]
        self.assertEqual(command[:2], [self.plan["subjects"]["cargo-clippy"]["path"], "clippy"])
        self.assertEqual(command[-len(REQUEST[1:]):], REQUEST[1:])
        self.assertIn("build.build-dir=" + json.dumps(str(self.paths["build"])), command)
        self.assertEqual(command[command.index("--target-dir") + 1], str(self.paths["target"]))
        self.assertEqual(command[command.index("--target") + 1], self.plan["host"])
        for name in ("CARGO", "RUSTC", "RUSTDOC", "SYSROOT", "RUSTC_WORKSPACE_WRAPPER", "CLIPPY_ARGS", "CLIPPY_CONF_DIR"):
            self.assertIn("env." + name + ".value=" + json.dumps(env[name]), command)
            self.assertIn("env." + name + ".force=true", command)
        self.assertEqual(env["CARGO"], self.plan["subjects"]["cargo"]["path"])
        self.assertEqual(env["RUSTC_WORKSPACE_WRAPPER"], self.plan["subjects"]["clippy-driver"]["path"])
        self.assertEqual(env["SYSROOT"], self.plan["root"])
        self.assertEqual(env["CARGO_INCREMENTAL"], "0")
        self.assertEqual(env["RUSTUP_AUTO_INSTALL"], "0")
        self.assertEqual(env["CLIPPY_ARGS"], safe.CLIPPY_LINT_ARGS)
        self.assertTrue(lock.exists(), "even lint success cannot release unverified descendants")
        self.assertIn('"lease_released": false', output)
        receipt = json.loads(next(line.split(": ", 1)[1] for line in output.splitlines()
                                  if line.startswith("cargo-admitted resources: ")))
        self.assertEqual(tuple(receipt["lease_identity"]), safe.directory_identity(lock))
        self.assertEqual(receipt["lease_marker"], str(next(lock.iterdir())))

    def test_success_failure_and_unfamiliar_exit_all_retain_exact_lease(self):
        for status in (0, 101, 17):
            with self.subTest(status=status):
                self.slot = self.root / ("slot-" + str(status))
                result, output = self.invoke(status=status)
                self.assertEqual(result, status)
                lock = self.slot / "cargo-active"
                self.assertTrue(lock.exists())
                marker = tuple(lock.iterdir())
                self.assertEqual(len(marker), 1)
                again, _ = self.invoke()
                self.assertEqual(again, 75)
                self.assertEqual(tuple(lock.iterdir()), marker)
                self.assertIn("awaiting owner verification", output)

    def test_only_finite_positive_work_shape_is_admitted(self):
        invalid = [["clippy"], ["clippy", "--version"], ["clippy", "--fix"],
                   ["cargo-clippy", *REQUEST[1:]], ["+1.95.0", *REQUEST],
                   REQUEST[:1] + ["--config=x"] + REQUEST[1:],
                   ["clippy", "-p", "*", *REQUEST[3:]],
                   ["clippy", "-p", "consumer@1.0", *REQUEST[3:]],
                   ["clippy", "-p", "consumer", "--profile", "dev", *REQUEST[6:]],
                   REQUEST[:-3] + ["--", "-A", "warnings"],
                   REQUEST[:-3] + ["--", "-D", "warnings", "--cap-lints", "allow"],
                   REQUEST[:1] + ["--help"] + REQUEST[1:],
                   REQUEST[:1] + ["--locked"] + REQUEST[1:]]
        for args in invalid:
            with self.subTest(args=args):
                result, _ = self.invoke(args)
                self.assertEqual(result, 75)
                self.assertFalse(self.slot.exists())

    def test_compiler_and_loader_injection_refuse_before_allocation(self):
        for name in ("CARGO", "RUSTC", "RUSTDOC", "CLIPPY_ARGS", "CLIPPY_CONF_DIR", "CLIPPY_DRIVER_PATH",
                     "SYSROOT", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS", "CARGO_BUILD_RUSTC",
                     "CARGO_BUILD_RUSTDOC", "CARGO_BUILD_TARGET", "LD_PRELOAD", "DYLD_LIBRARY_PATH"):
            with self.subTest(name=name):
                result, _ = self.invoke(env=dict(self.env, **{name: "foreign"}))
                self.assertEqual(result, 75)
                self.assertFalse(self.slot.exists())

    def test_preflight_binds_toolchain_and_config_without_launch_or_lease(self):
        with patch.object(safe, "call_clippy") as launch:
            result, output = self.invoke(["--preflight", *REQUEST])
            launch.assert_not_called()
        self.assertEqual(result, 0)
        report = json.loads(output.split("cargo-admitted preflight: ")[1])
        self.assertEqual(report["scope"]["clippy_setup"]["toolchain"], self.plan)
        self.assertEqual(report["scope"]["clippy_setup"]["configuration"], [])
        self.assertFalse(self.slot.exists())

    def test_budget_cannot_transfer_to_changed_toolchain_or_configuration(self):
        _, output = self.invoke(["--preflight", *REQUEST], config=Mock(return_value=[{"sha256": "original"}]))
        scope = json.loads(output.split("cargo-admitted preflight: ")[1])["scope"]
        budget = self.root / "budget.json"
        budget.write_text(json.dumps({"schema_version": 1, "scope": scope, "reserve_bytes": 1024,
                                     "expected_growth_bytes": 1024, "basis": "fixture only"}))
        # Capacity knobs conflict with budget files; this fixture deliberately
        # removes them rather than weakening the production defaults.
        result, _ = self.invoke(["--budget-file", str(budget), *REQUEST], env={"FIXTURE": "yes"},
                                config=Mock(return_value=[{"sha256": "changed"}]))
        self.assertEqual(result, 75)
        self.assertFalse(self.slot.exists())
        self.plan["subjects"]["cargo"]["path"] = str(self.root / "different-cargo")
        result, _ = self.invoke(["--budget-file", str(budget), *REQUEST], env={"FIXTURE": "yes"},
                                config=Mock(return_value=[{"sha256": "original"}]))
        self.assertEqual(result, 75)
        self.assertFalse(self.slot.exists())

    def test_changed_configuration_refuses_and_releases_only_unlaunched_lease(self):
        result, output = self.invoke(config=Mock(side_effect=[[], [{"path": "changed"}]]))
        self.assertEqual(result, 75)
        self.assertFalse((self.slot / "cargo-active").exists())
        self.assertNotIn("Clippy product:", output)

    def test_setup_failure_retains_owned_lease_without_product_verdict(self):
        result, output = self.invoke(versions=Mock(side_effect=safe.Denied("wrong toolchain")))
        self.assertEqual(result, 75)
        self.assertTrue((self.slot / "cargo-active").exists())
        self.assertNotIn("Clippy product:", output)
        receipt = json.loads(next(line.split(": ", 1)[1] for line in output.splitlines()
                                  if line.startswith("cargo-admitted resources: ")))
        lock = Path(receipt["lease"])
        self.assertTrue(safe.owns_lease(lock, tuple(receipt["lease_identity"]), Path(receipt["lease_marker"])))

    def test_configuration_change_during_setup_refuses_package_and_retains_lease(self):
        launch = Mock()
        result, output = self.invoke(call=launch, config=Mock(side_effect=[[], [], [{"path": "changed"}]]))
        self.assertEqual(result, 75)
        launch.assert_not_called()
        self.assertTrue((self.slot / "cargo-active").exists())
        self.assertNotIn("Clippy product:", output)

    def test_cargo_loader_entries_and_includes_refuse_before_scope(self):
        worktree = self.root / "worktree"
        (worktree / ".cargo").mkdir(parents=True)
        config = worktree / ".cargo/config.toml"
        for text in ('include = ["extra.toml"]\n',
                     '[env]\nLD_PRELOAD = {value="/foreign.so",force=true}\n',
                     '[env]\n"LD_LIBRARY_PATH" = "/foreign"\n',
                     'env."DYLD_INSERT_LIBRARIES" = "/foreign"\n'):
            with self.subTest(text=text), patch.object(safe.Path, "cwd", return_value=worktree):
                config.write_text(text)
                with self.assertRaises(safe.Denied):
                    safe.clippy_configuration(worktree, self.paths)

    def test_interruption_retains_owned_lease(self):
        with self.assertRaises(KeyboardInterrupt):
            self.invoke(call=Mock(side_effect=KeyboardInterrupt))
        self.assertTrue((self.slot / "cargo-active").exists())


class ClippyIdentityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="clippy-identity-controls-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.worktree = self.root / "worktree"
        self.worktree.mkdir()
        (self.worktree / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.95.0"\n')
        self.home = self.root / "rustup"
        self.tools = self.home / "toolchains/1.95.0-x86_64-unknown-linux-gnu"
        (self.tools / "bin").mkdir(parents=True)
        (self.tools / "lib").mkdir()
        for name in ("cargo", "rustc", "rustdoc", "cargo-clippy", "clippy-driver"):
            (self.tools / "bin" / name).write_text(name)
        (self.tools / "lib/librustc_driver-fixture.so").write_text("fixture runtime")
        self.env = {"RUSTUP_HOME": str(self.home)}

    def plan(self):
        with patch.object(safe.sys, "platform", "linux"), patch.object(safe.platform, "machine", return_value="x86_64"):
            return safe.clippy_toolchain(self.env, self.worktree)

    def test_direct_installed_files_and_runtime_are_bound(self):
        plan = self.plan()
        self.assertEqual(plan["host"], "x86_64-unknown-linux-gnu")
        self.assertEqual(plan["subjects"]["cargo"]["path"], str(self.tools / "bin/cargo"))
        self.assertIn("runtime-0", plan["subjects"])
        safe.revalidate_clippy_toolchain(plan)

    def test_changed_executable_or_pin_invalidates_frozen_identity(self):
        plan = self.plan()
        (self.tools / "bin/cargo-clippy").write_text("changed")
        with self.assertRaises(safe.Denied):
            safe.revalidate_clippy_toolchain(plan)
        plan = self.plan()
        (self.worktree / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.99.0"\n')
        with self.assertRaises(safe.Denied):
            safe.revalidate_clippy_toolchain(plan)

    def test_missing_tool_or_foreign_toolchain_refuses_without_installation(self):
        self.env["RUSTUP_TOOLCHAIN"] = "nightly"
        with self.assertRaises(safe.Denied):
            self.plan()
        self.env.pop("RUSTUP_TOOLCHAIN")
        (self.tools / "bin/clippy-driver").unlink()
        with self.assertRaises(OSError):
            self.plan()

    def test_mismatched_driver_commit_is_setup_failure(self):
        plan = self.plan()
        commit = "a" * 40
        outputs = [b"cargo 1.95.0 (fixture)",
                   ("rustc 1.95.0\nrelease: 1.95.0\nhost: x86_64-unknown-linux-gnu\ncommit-hash: " + commit + "\n").encode(),
                   b"clippy 0.1.95 (bbbbbbbbbb 2026-04-14)", b"clippy 0.1.95 (bbbbbbbbbb 2026-04-14)"]
        results = [subprocess.CompletedProcess([], 0, stdout=out, stderr=b"") for out in outputs]
        with patch.object(safe.subprocess, "run", side_effect=results), self.assertRaises(safe.Denied):
            safe.clippy_version_check(plan, {})

    def test_cancellation_signals_owned_unreaped_group_and_restores_handlers(self):
        process = Mock(pid=98765, returncode=None)
        handlers = {}
        def install(signum, handler):
            old = handlers.get(signum, "old")
            handlers[signum] = handler
            return old
        def interrupted(**kwargs):
            handlers[signal.SIGINT](signal.SIGINT, None)
        process.wait.side_effect = interrupted
        with patch.object(safe.subprocess, "Popen", return_value=process), \
             patch.object(safe.signal, "signal", side_effect=install), \
             patch.object(safe.os, "killpg") as kill, contextlib.redirect_stderr(io.StringIO()), \
             self.assertRaises(KeyboardInterrupt):
            safe.call_clippy(["fixture"], {}, self.root / "lease")
        kill.assert_called_once_with(98765, signal.SIGTERM)
        self.assertEqual(handlers[signal.SIGINT], "old")
        self.assertEqual(handlers[signal.SIGTERM], "old")

    def test_late_cancellation_never_signals_a_reaped_numeric_group(self):
        process = Mock(pid=98765, returncode=0)
        handlers = {}
        def install(signum, handler):
            handlers[signum] = handler
            return "old"
        def completed(**kwargs):
            handlers[signal.SIGINT](signal.SIGINT, None)
        process.wait.side_effect = completed
        with patch.object(safe.subprocess, "Popen", return_value=process), \
             patch.object(safe.signal, "signal", side_effect=install), \
             patch.object(safe.os, "killpg") as kill, contextlib.redirect_stderr(io.StringIO()), \
             self.assertRaises(KeyboardInterrupt):
            safe.call_clippy(["fixture"], {}, self.root / "lease")
        kill.assert_not_called()

    def test_cancellation_during_spawn_is_deferred_until_identity_is_published(self):
        process = Mock(pid=98765, returncode=None)
        handlers = {}
        output = io.StringIO()
        def install(signum, handler):
            old = handlers.get(signum, "old")
            handlers[signum] = handler
            return old
        def spawn(*args, **kwargs):
            self.assertTrue(kwargs["start_new_session"])
            handlers[signal.SIGTERM](signal.SIGTERM, None)
            return process
        def terminate(*args):
            self.assertIn('"process_group": 98765', output.getvalue())
        with patch.object(safe.subprocess, "Popen", side_effect=spawn), \
             patch.object(safe.signal, "signal", side_effect=install), \
             patch.object(safe.os, "killpg", side_effect=terminate) as kill, \
             contextlib.redirect_stderr(output), self.assertRaises(KeyboardInterrupt):
            safe.call_clippy(["fixture"], {}, self.root / "lease")
        kill.assert_called_once_with(98765, signal.SIGTERM)
        process.wait.assert_not_called()
        self.assertEqual(handlers[signal.SIGTERM], "old")


if __name__ == "__main__":
    unittest.main()
