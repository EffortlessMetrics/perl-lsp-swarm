#!/usr/bin/env python3
"""Compiler-free CLI proof for the historical policy validator's build route.

The real compatibility script runs in a disposable checkout layout. Its admitted
delegate records argv, cwd and selected environment values; a raw Cargo trap
prevents these routing tests from compiling anything. Admission and Rust policy
validation themselves remain with their existing owners and are not proved here.
"""

from __future__ import annotations

import json
import os
from pathlib import Path, PureWindowsPath
import subprocess
import runpy
import sys
import tempfile
import unittest
from unittest import mock


SOURCE = Path(__file__).resolve().parents[1] / "policy" / "validate_non_rust_allowlist.py"
ENV_KEYS = (
    "CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "CARGO_HOME", "DEVPLANE",
    "TMPDIR", "TEMP", "TMP", "RUSTFLAGS", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN",
    "RUSTUP_AUTO_INSTALL", "POLICY_ROUTE_SENTINEL",
)

DELEGATE = r'''import json
import os
from pathlib import Path
import sys

args = sys.argv[1:]
contents = {}
for flag in ("--allowlist", "--debt"):
    if flag in args:
        path = Path(args[args.index(flag) + 1])
        contents[flag] = path.read_text() if path.is_file() else None
record = {"argv": args, "cwd": os.getcwd(), "interpreter": sys.executable,
          "environment": {key: os.environ.get(key) for key in
                          json.loads(os.environ["POLICY_ROUTE_KEYS"])},
          "contents": contents, "bytecode_disabled": sys.dont_write_bytecode}
with open(os.environ["POLICY_ROUTE_RECORD"], "a", encoding="utf-8") as handle:
    handle.write(json.dumps(record) + "\n")
print("validator stdout")
print("validator stderr", file=sys.stderr)
sys.exit(int(os.environ["POLICY_ROUTE_EXIT"]))
'''


class PolicyValidatorBuildRoutingTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory(prefix="policy-build-routing-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.repo = self.root / "candidate repo"
        self.script = self.repo / "scripts" / "policy" / SOURCE.name
        self.script.parent.mkdir(parents=True)
        self.script.write_bytes(SOURCE.read_bytes())
        (self.repo / "scripts" / "cargo_admitted.py").write_text(DELEGATE)
        self.outside = self.root / "outside caller"
        self.outside.mkdir()
        self.nested = self.repo / "nested caller"
        self.nested.mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        trap = self.bin / "cargo"
        trap.write_text(
            f"#!{sys.executable}\n"
            "import os\nfrom pathlib import Path\n"
            "Path(os.environ['POLICY_ROUTE_RAW']).write_text('raw Cargo invoked')\n"
            "raise SystemExit(92)\n"
        )
        trap.chmod(0o755)
        if os.name == "nt":
            (self.bin / "cargo.cmd").write_text(
                f'@"{sys.executable}" "{trap}" %*\r\n'
            )
        self.invocation = 0

    def invoke(self, args=(), *, cwd=None, status=0, inherited=None):
        self.invocation += 1
        receipt = self.root / f"receipt-{self.invocation}.jsonl"
        raw = self.root / f"raw-{self.invocation}.txt"
        env = os.environ.copy()
        for key in ENV_KEYS:
            env.pop(key, None)
        env.update(
            PATH=str(self.bin),
            POLICY_ROUTE_RECORD=str(receipt),
            POLICY_ROUTE_RAW=str(raw),
            POLICY_ROUTE_KEYS=json.dumps(ENV_KEYS),
            POLICY_ROUTE_EXIT=str(status),
        )
        env.update(inherited or {})
        directories = {str(path.relative_to(self.root)) for path in self.root.rglob("*") if path.is_dir()}
        result = subprocess.run(
            [sys.executable, "-B", str(self.script), *args],
            cwd=cwd or self.repo,
            env=env,
            text=True,
            capture_output=True,
            timeout=10,
            check=False,
        )
        records = [json.loads(line) for line in receipt.read_text().splitlines()] if receipt.exists() else []
        self.assertFalse(raw.exists(), "compatibility entrypoint invoked raw Cargo")
        self.assertEqual(
            directories,
            {str(path.relative_to(self.root)) for path in self.root.rglob("*") if path.is_dir()},
            "routing created storage or bytecode directories",
        )
        return result, records

    def assert_dispatch(self, result, records, allowlist, debt, *, status=0):
        self.assertEqual(result.returncode, status, result.stderr)
        self.assertEqual(len(records), 1, "must dispatch the admitted owner exactly once")
        record = records[0]
        self.assertEqual(record["argv"], [
            "run", "-p", "xtask", "--bin", "xtask", "--locked", "--",
            "non-rust", "validate-policy", "--allowlist", str(allowlist),
            "--debt", str(debt),
        ])
        self.assertEqual(Path(record["cwd"]), self.repo)
        self.assertEqual(record["interpreter"], sys.executable)
        self.assertTrue(record["bytecode_disabled"])
        self.assertEqual(result.stdout, "validator stdout\n")
        self.assertEqual(result.stderr, "validator stderr\n")
        return record

    def test_default_paths_keep_each_callers_referents(self):
        for caller in (self.repo, self.nested, self.outside):
            with self.subTest(caller=caller.name):
                policy = caller / "policy"
                policy.mkdir()
                allowlist = policy / "non-rust-allowlist.toml"
                debt = policy / "non-rust-debt.toml"
                allowlist.write_text(f"allowlist in {caller.name}")
                debt.write_text(f"debt in {caller.name}")
                result, records = self.invoke(cwd=caller)
                record = self.assert_dispatch(result, records, allowlist, debt)
                self.assertEqual(record["contents"], {
                    "--allowlist": allowlist.read_text(), "--debt": debt.read_text(),
                })

    def test_literal_relative_paths_are_not_shell_source(self):
        allow = "policy $(touch injected) ; [proof] ' name.toml"
        debt = "--debt file.toml"
        (self.outside / allow).write_text("literal allowlist")
        (self.outside / debt).write_text("literal debt")
        result, records = self.invoke(
            ["--allowlist", allow, f"--debt={debt}"], cwd=self.outside,
        )
        record = self.assert_dispatch(result, records, self.outside / allow, self.outside / debt)
        self.assertEqual(record["contents"], {"--allowlist": "literal allowlist", "--debt": "literal debt"})
        self.assertFalse((self.outside / "injected").exists())

    def test_absolute_policy_paths_are_preserved(self):
        allow = self.outside / "allow.toml"
        debt = self.outside / "debt.toml"
        result, records = self.invoke(["--allowlist", str(allow), "--debt", str(debt)], cwd=self.nested)
        self.assert_dispatch(result, records, allow, debt)

    def test_symlink_parent_segments_keep_filesystem_meaning(self):
        destination = self.root / "real location"
        (destination / "child").mkdir(parents=True)
        (destination / "allow.toml").write_text("filesystem parent")
        (self.outside / "allow.toml").write_text("wrong lexical parent")
        try:
            (self.outside / "link").symlink_to(destination / "child", target_is_directory=True)
        except OSError as error:
            self.skipTest(f"directory symlink unavailable: {error}")
        relative = Path("link") / ".." / "allow.toml"
        result, records = self.invoke(["--allowlist", str(relative)], cwd=self.outside)
        record = self.assert_dispatch(
            result, records, self.outside / relative, self.outside / "policy/non-rust-debt.toml",
        )
        self.assertEqual(record["contents"]["--allowlist"], "filesystem parent")
        self.assertIn("..", record["argv"][-3])

    def test_no_storage_or_toolchain_environment_is_invented(self):
        result, records = self.invoke()
        record = self.assert_dispatch(
            result, records, self.repo / "policy/non-rust-allowlist.toml",
            self.repo / "policy/non-rust-debt.toml",
        )
        self.assertEqual(record["environment"], dict.fromkeys(ENV_KEYS))

    def test_inherited_overrides_reach_the_admission_owner_unchanged(self):
        inherited = {key: f"preserve {key}" for key in ENV_KEYS}
        result, records = self.invoke(inherited=inherited, status=75)
        record = self.assert_dispatch(
            result, records, self.repo / "policy/non-rust-allowlist.toml",
            self.repo / "policy/non-rust-debt.toml", status=75,
        )
        self.assertEqual(record["environment"], inherited)

    def test_delegate_failures_and_refusals_are_not_retried(self):
        for status in (0, 37, 75):
            with self.subTest(status=status):
                result, records = self.invoke(status=status)
                self.assert_dispatch(
                    result, records, self.repo / "policy/non-rust-allowlist.toml",
                    self.repo / "policy/non-rust-debt.toml", status=status,
                )

    def test_windows_drive_relative_paths_are_frozen_before_changing_cwd(self):
        # Portable path-contract control, not a native Windows execution claim.
        # Each drive has its own current directory. Keep the tail lexical so a
        # symlink followed by '..' is still interpreted by the filesystem.
        class WindowsPathOracle(PureWindowsPath):
            @classmethod
            def cwd(cls):
                return cls(r"C:\outside")

            def resolve(self):
                return type(self)(r"D:\checkout\scripts\policy\validate_non_rust_allowlist.py")

            def absolute(self):
                if self.is_absolute():
                    return self
                # Match Python 3.11: a different drive remains relative.
                return self.cwd() / self

        module = runpy.run_path(str(SOURCE), run_name="policy_route_contract")
        args = type("Arguments", (), {
            "allowlist": WindowsPathOracle(r"D:link\..\allow.toml"),
            "debt": WindowsPathOracle(r"policy\debt.toml"),
        })()
        with mock.patch("argparse.ArgumentParser.parse_args", return_value=args), \
             mock.patch("pathlib.Path", WindowsPathOracle), \
             mock.patch("os.path.abspath", return_value=r"D:\original-context") as drive_cwd, \
             mock.patch("subprocess.run") as dispatch:
            dispatch.return_value.returncode = 75
            self.assertEqual(module["main"](), 75)
        drive_cwd.assert_called_once_with("D:")
        dispatch.assert_called_once()
        command = dispatch.call_args.args[0]
        self.assertEqual(command[-3], r"D:\original-context\link\..\allow.toml")
        self.assertEqual(command[-1], r"C:\outside\policy\debt.toml")
        self.assertEqual(str(dispatch.call_args.kwargs["cwd"]), r"D:\checkout")

    def test_help_needs_no_build_or_admission(self):
        result, records = self.invoke(["--help"], cwd=self.outside)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--allowlist", result.stdout)
        self.assertIn("--debt", result.stdout)
        self.assertEqual(records, [])

    def test_missing_admission_owner_does_not_fallback(self):
        (self.repo / "scripts" / "cargo_admitted.py").unlink()
        result, records = self.invoke(cwd=self.outside)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("cargo_admitted.py", result.stderr)
        self.assertEqual(records, [])

    def test_argument_errors_never_dispatch(self):
        for args in (["--unknown"], ["--allowlist"], ["--debt"]):
            with self.subTest(args=args):
                result, records = self.invoke(args, cwd=self.outside)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertIn("error:", result.stderr)
                self.assertEqual(records, [])


if __name__ == "__main__":
    unittest.main()
