"""Clippy setup/dispatch prerequisite, NOT an admitted package-proof route.

Default tests preserve production refusal. Opt in to the real, installed 1.95
driver with CLIPPY_CONTRACT_TOOLCHAIN_ROOT=/absolute/toolchain/root. It launches
only a recording fixture instead of Cargo: no compilation, downloads or policy
changes. Native Windows dispatch remains NOT_PROVEN (the spy is a POSIX script).
"""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("cargo_admitted", ROOT / "scripts/cargo_admitted.py")
admitted = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(admitted)


class ProductionRefusalTests(unittest.TestCase):
    def test_clippy_and_external_spellings_refuse_before_resources_or_spawn(self):
        for args in (["clippy", "-p", "consumer"], ["cargo-clippy", "clippy"],
                     ["+1.95.0", "clippy"], ["c", "-p", "consumer"],
                     ["--preflight", "clippy", "-p", "consumer"]):
            with self.subTest(args=args), contextlib.redirect_stderr(io.StringIO()), \
                 patch.object(admitted, "resource_plan") as resources, \
                 patch.object(admitted.subprocess, "call") as launch:
                self.assertEqual(admitted.main(list(args)), 75)
                resources.assert_not_called()
                launch.assert_not_called()

    def test_existing_configuration_and_path_refusals_still_apply(self):
        for arg in ("--config=x", "--target-dir=x", "--build-dir=x",
                    "--manifest-path=x", "-j8", "--artifact-dir=x"):
            with self.subTest(arg=arg), self.assertRaises(admitted.Denied):
                admitted.validate_args(["check", arg])


TOOLCHAIN_ROOT = os.environ.get("CLIPPY_CONTRACT_TOOLCHAIN_ROOT")


@unittest.skipUnless(TOOLCHAIN_ROOT and os.name == "posix",
                     "explicit installed toolchain + POSIX required; native Windows NOT_PROVEN")
class InstalledDriverDispatchTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # This observation is deliberately test-local, not a provisioning or
        # executable-identity authority for a production executor.
        root = Path(TOOLCHAIN_ROOT)
        if not root.is_absolute() or root.is_symlink():
            raise AssertionError("provide the absolute installed toolchain root, not a proxy")
        cls.driver = root / "bin" / "cargo-clippy"
        cls.wrapper = root / "bin" / "clippy-driver"
        cls.probe_env = dict(os.environ, RUSTUP_AUTO_INSTALL="0")
        versions = []
        for name in ("cargo", "rustc", "cargo-clippy", "clippy-driver"):
            executable = root / "bin" / name
            if not executable.is_file() or executable.is_symlink():
                raise AssertionError("missing/direct executable required: " + str(executable))
            result = subprocess.run([str(executable), "--version"], env=cls.probe_env,
                                    capture_output=True, text=True, timeout=15, check=True)
            versions.append(result.stdout.strip())
        if not versions[0].startswith("cargo 1.95.0 ") or not versions[1].startswith("rustc 1.95.0 "):
            raise AssertionError("this upstream dispatch packet is pinned to Rust/Cargo 1.95.0")
        if versions[2] != versions[3] or not versions[2].startswith("clippy 0.1.95 "):
            raise AssertionError("cargo-clippy/clippy-driver must match the pinned toolchain")

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="clippy-dispatch-contract-")
        self.addCleanup(self.temp.cleanup)
        self.fixture = Path(self.temp.name) / "space ü fixture"
        self.fixture.mkdir()
        self.receipt = self.fixture / "dispatch.json"
        self.spy = self.fixture / "cargo-spy"
        self.spy.write_text("#!" + sys.executable + "\n" +
                            "import json, os, pathlib, sys\n"
                            "keys = ('CARGO', 'RUSTC_WORKSPACE_WRAPPER', 'CLIPPY_ARGS', "
                            "'CARGO_TARGET_DIR', 'CARGO_BUILD_BUILD_DIR', 'CARGO_INCREMENTAL', "
                            "'CARGO_BUILD_JOBS', 'TMPDIR', 'TEMP', 'TMP', 'RUSTUP_AUTO_INSTALL')\n"
                            "pathlib.Path(os.environ['SPY_RECEIPT']).write_text(json.dumps({"
                            "'argv': sys.argv[1:], 'cwd': os.getcwd(), "
                            "'env': {k: os.environ.get(k) for k in keys}}))\n"
                            "sys.exit(int(os.environ.get('SPY_EXIT', '0')))\n", encoding="utf-8")
        self.spy.chmod(0o700)
        # Explicit source-free fixture process. Nothing here is a production
        # environment projection or a host-capacity admission.
        self.env = dict(self.probe_env, CARGO=str(self.spy), SPY_RECEIPT=str(self.receipt),
                        CARGO_TARGET_DIR=str(self.fixture / "private target"),
                        CARGO_BUILD_BUILD_DIR=str(self.fixture / "private build"),
                        CARGO_INCREMENTAL="0", CARGO_BUILD_JOBS="2", RUSTUP_AUTO_INSTALL="0",
                        TMPDIR=str(self.fixture), TEMP=str(self.fixture), TMP=str(self.fixture),
                        RUSTC_WORKSPACE_WRAPPER="untrusted ambient wrapper", CLIPPY_ARGS="ambient lint")

    def dispatch(self, args, status=0):
        self.receipt.unlink(missing_ok=True)
        result = subprocess.run([str(self.driver), *args], cwd=self.fixture,
                                env=dict(self.env, SPY_EXIT=str(status)),
                                capture_output=True, text=True, timeout=15)
        record = json.loads(self.receipt.read_text()) if self.receipt.exists() else None
        return result, record

    def test_exact_package_profile_paths_and_lints_reach_child_cargo(self):
        cargo_args = ["-p", "consumer", "--all-targets", "--profile", "agent", "--locked",
                      "--offline", "--target-dir", self.env["CARGO_TARGET_DIR"],
                      "--config", "build.build-dir=" + json.dumps(self.env["CARGO_BUILD_BUILD_DIR"])]
        result, record = self.dispatch(["clippy", *cargo_args, "--", "-D", "warnings"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIsNotNone(record, "zero-exit setup is not dispatch evidence")
        self.assertEqual(record["argv"], ["check", *cargo_args])
        self.assertEqual(record["cwd"], str(self.fixture))
        self.assertEqual(record["env"]["RUSTC_WORKSPACE_WRAPPER"], str(self.wrapper))
        self.assertEqual(record["env"]["CLIPPY_ARGS"], "-D__CLIPPY_HACKERY__warnings__CLIPPY_HACKERY__")
        for key in ("CARGO", "CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "CARGO_INCREMENTAL",
                    "CARGO_BUILD_JOBS", "TMPDIR", "TEMP", "TMP", "RUSTUP_AUTO_INSTALL"):
            self.assertEqual(record["env"][key], self.env[key], key)

    def test_child_exit_is_preserved_without_product_or_terminality_claim(self):
        for status in (0, 101, 17):
            with self.subTest(status=status):
                result, record = self.dispatch(["clippy", "-p", "consumer"], status)
                self.assertIsNotNone(record)
                self.assertEqual(result.returncode, status)

    def test_missing_subcommand_token_silently_loses_first_cargo_argument(self):
        result, record = self.dispatch(["-p", "consumer", "--locked"])
        self.assertEqual(result.returncode, 0)
        self.assertEqual(record["argv"], ["check", "consumer", "--locked"])
        self.assertNotEqual(record["argv"], ["check", "-p", "consumer", "--locked"])

    def test_global_config_before_subcommand_is_not_a_cargo_global_option(self):
        _, record = self.dispatch(["--config", "build.jobs=2", "clippy", "-p", "consumer"])
        self.assertEqual(record["argv"], ["check", "build.jobs=2", "clippy", "-p", "consumer"])

    def test_fix_is_a_different_mutating_operation(self):
        _, record = self.dispatch(["clippy", "--fix", "-p", "consumer"])
        self.assertEqual(record["argv"], ["fix", "-p", "consumer"])
        self.assertEqual(record["env"]["CLIPPY_ARGS"], "--no-deps__CLIPPY_HACKERY__")

    def test_no_deps_is_driver_input_not_a_cargo_argument(self):
        _, record = self.dispatch(["clippy", "--no-deps", "-p", "consumer", "--", "-D", "warnings"])
        self.assertEqual(record["argv"], ["check", "-p", "consumer"])
        self.assertEqual(record["env"]["CLIPPY_ARGS"],
                         "--no-deps__CLIPPY_HACKERY__-D__CLIPPY_HACKERY__warnings__CLIPPY_HACKERY__")

    def test_version_can_return_success_without_any_selected_work(self):
        result, record = self.dispatch(["clippy", "-p", "consumer", "--version"])
        self.assertEqual(result.returncode, 0)
        self.assertIsNone(record)
        self.assertTrue(result.stdout.startswith("clippy 0.1.95 "))

    def test_unset_cargo_uses_ambient_path_instead_of_toolchain_sibling(self):
        self.env.pop("CARGO")
        ambient = self.fixture / "cargo"
        ambient.write_bytes(self.spy.read_bytes())
        ambient.chmod(0o700)
        self.env["PATH"] = str(self.fixture)
        _, record = self.dispatch(["clippy", "-p", "consumer"])
        self.assertIsNotNone(record, "ambient PATH fixture must execute")
        self.assertIsNone(record["env"]["CARGO"])
        self.assertEqual(record["argv"], ["check", "-p", "consumer"])


if __name__ == "__main__":
    unittest.main()
