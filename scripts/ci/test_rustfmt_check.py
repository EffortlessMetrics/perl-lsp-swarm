#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("rustfmt_check.py")
SPEC = importlib.util.spec_from_file_location("rustfmt_check", MODULE_PATH)
assert SPEC and SPEC.loader
rustfmt_check = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = rustfmt_check
SPEC.loader.exec_module(rustfmt_check)

class RustfmtCheckTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = (Path(self.temp.name) / "workspace").resolve()
        self.root.mkdir()
        self.control = (Path(self.temp.name) / "control").resolve()
        self.control.mkdir()
        self.receipt = self.root / "receipt.json"
        self.bin = self.control / "fake-bin"
        self.bin.mkdir()
        self._write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"pkg-a\", \"xtask\"]\nresolver = \"2\"\n",
        )
        self._write("Cargo.lock", "# lock\n")
        self._write("rust-toolchain.toml", "[toolchain]\nchannel = \"1.95.0\"\n")
        self._write("rustfmt.toml", "max_width = 100\n")
        self._write(".gitignore", "receipt*.json\ntarget/\n")
        self._write("pkg-a/Cargo.toml", "[package]\nname = \"pkg-a\"\nversion = \"0.1.0\"\n")
        self._write("pkg-a/src/lib.rs", "pub fn value() -> u8 { 1 }\n")
        self._write("xtask/Cargo.toml", "[package]\nname = \"xtask\"\nversion = \"0.1.0\"\n")
        self._write("xtask/src/main.rs", "fn main() {}\n")
        self._write("xtask/tests/format_me.rs", "#[test]\nfn formats() {}\n")
        self.metadata = self.control / "metadata.json"
        self._write_metadata()
        self.config = self.control / "fmt-config.json"
        self.config.write_text("{}\n", encoding="utf-8")
        self.cargo = self.bin / ("cargo.cmd" if os.name == "nt" else "cargo")
        self.rustfmt = self.bin / ("rustfmt.cmd" if os.name == "nt" else "rustfmt")
        self.rustc = self.bin / ("rustc.cmd" if os.name == "nt" else "rustc")
        self._write_fake_cargo()
        self._write_fake_rustfmt()
        self._write_fake_rustc()
        self._git("init", "--initial-branch=main")
        self._git("config", "user.name", "rustfmt fixture")
        self._git("config", "user.email", "rustfmt-fixture@example.invalid")
        self._git("add", ".")
        self._git("commit", "-m", "fixture")
        self.candidate_sha = self._git("rev-parse", "HEAD").stdout.strip()
        self.tree_sha = self._git("rev-parse", "HEAD^{tree}").stdout.strip()

    def tearDown(self) -> None:
        self.temp.cleanup()

    def _write(self, relative: str, content: str) -> Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        return path

    def _git(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                "git",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=",
                "-c",
                "init.templateDir=",
                *args,
            ],
            cwd=self.root,
            text=True,
            capture_output=True,
            check=True,
        )

    def _package_id(self, name: str) -> str:
        return f"{name} 0.1.0 (path+file://{self.root / name})"

    def _write_metadata(self, *, manifest_override: str | None = None) -> None:
        pkg_manifest = manifest_override or str(self.root / "pkg-a/Cargo.toml")
        payload = {
            "workspace_root": str(self.root),
            "workspace_members": [self._package_id("pkg-a"), self._package_id("xtask")],
            "packages": [
                {
                    "id": self._package_id("pkg-a"),
                    "name": "pkg-a",
                    "manifest_path": pkg_manifest,
                    "targets": [
                        {
                            "name": "pkg_a",
                            "kind": ["lib"],
                            "src_path": str(self.root / "pkg-a/src/lib.rs"),
                        }
                    ],
                },
                {
                    "id": self._package_id("xtask"),
                    "name": "xtask",
                    "manifest_path": str(self.root / "xtask/Cargo.toml"),
                    "targets": [
                        {
                            "name": "xtask",
                            "kind": ["bin"],
                            "src_path": str(self.root / "xtask/src/main.rs"),
                        },
                        {
                            "name": "format_me",
                            "kind": ["test"],
                            "src_path": str(self.root / "xtask/tests/format_me.rs"),
                        },
                    ],
                },
            ],
        }
        self.metadata.write_text(json.dumps(payload), encoding="utf-8")

    def _write_fake_cargo(self) -> None:
        script = r'''#!/usr/bin/env python3
import json
import os
import pathlib
import subprocess
import sys
import time

args = sys.argv[1:]
if args == ["--version"]:
    print("cargo 1.95.0 (fixture)")
    raise SystemExit(0)
if args and args[0] == "metadata":
    if "--locked" not in args:
        print("metadata must be locked", file=sys.stderr)
        raise SystemExit(8)
    if os.environ.get("FAKE_METADATA_FAIL") == "1":
        print("metadata failed", file=sys.stderr)
        raise SystemExit(9)
    print(pathlib.Path(os.environ["FAKE_METADATA"]).read_text(encoding="utf-8"))
    raise SystemExit(0)
if args and args[0] == "fmt":
    expected_rustfmt = os.environ.get("EXPECT_RUSTFMT")
    if expected_rustfmt and os.environ.get("RUSTFMT") != expected_rustfmt:
        print("cargo did not receive the selected rustfmt", file=sys.stderr)
        raise SystemExit(10)
    manifest = pathlib.Path(args[args.index("--manifest-path") + 1])
    root = pathlib.Path(os.environ["FAKE_ROOT"]).resolve()
    if not manifest.is_absolute():
        manifest = root / manifest
    key = manifest.resolve().relative_to(root).as_posix()
    config = json.loads(pathlib.Path(os.environ["FAKE_FMT_CONFIG"]).read_text(encoding="utf-8"))
    result = config.get(key, {})
    time.sleep(float(result.get("sleep", 0)))
    if result.get("checkout"):
        subprocess.run(
            ["git", "checkout", "--detach", result["checkout"]],
            cwd=root,
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    if result.get("stdout"):
        print(result["stdout"])
    if result.get("stderr"):
        print(result["stderr"], file=sys.stderr)
    raise SystemExit(int(result.get("exit", 0)))
print(f"unexpected cargo invocation: {args}", file=sys.stderr)
raise SystemExit(64)
'''
        self._write_fake_executable(self.cargo, script)

    def _write_fake_rustfmt(self) -> None:
        script = r'''#!/usr/bin/env python3
import os
import sys
import time
if os.environ.get("FAKE_RUSTFMT_FAIL") == "1":
    print("rustfmt unavailable", file=sys.stderr)
    raise SystemExit(7)
time.sleep(float(os.environ.get("FAKE_RUSTFMT_SLEEP", "0")))
if sys.argv[1:] == ["--version"]:
    print("rustfmt 1.95.0-stable (fixture)")
    raise SystemExit(0)
raise SystemExit(64)
'''
        self._write_fake_executable(self.rustfmt, script)

    def _write_fake_rustc(self) -> None:
        script = r'''#!/usr/bin/env python3
import sys
if sys.argv[1:] == ["-Vv"]:
    print("rustc 1.95.0 (fixture 2026-08-01)")
    print("binary: rustc")
    print("commit-hash: 0123456789abcdef0123456789abcdef01234567")
    print("commit-date: 2026-08-01")
    print("host: x86_64-unknown-linux-gnu")
    print("release: 1.95.0")
    print("LLVM version: 20.1.0")
    raise SystemExit(0)
raise SystemExit(64)
'''
        self._write_fake_executable(self.rustc, script)

    def _write_fake_executable(self, path: Path, script: str) -> None:
        if os.name == "nt":
            implementation = path.with_suffix(".py")
            implementation.write_text(script, encoding="utf-8")
            path.write_text(
                f'@"{sys.executable}" "{implementation}" %*\n',
                encoding="utf-8",
            )
        else:
            path.write_text(script, encoding="utf-8")
            path.chmod(0o755)

    def set_fmt(self, mapping: dict[str, dict[str, object]]) -> None:
        self.config.write_text(json.dumps(mapping), encoding="utf-8")

    def run_check(
        self,
        *,
        receipt: Path | None = None,
        timeout: float = 10.0,
        extra_env: dict[str, str] | None = None,
        rustfmt: Path | None = None,
        max_output: int = 16384,
        max_findings: int = 1024,
    ) -> subprocess.CompletedProcess[str]:
        env = {"PATH": os.environ.get("PATH", "")}
        if os.name == "nt" and os.environ.get("SYSTEMROOT"):
            env["SYSTEMROOT"] = os.environ["SYSTEMROOT"]
        env.update(
            {
                "FAKE_METADATA": str(self.metadata),
                "FAKE_FMT_CONFIG": str(self.config),
                "FAKE_ROOT": str(self.root),
            }
        )
        if extra_env:
            env.update(extra_env)
        return subprocess.run(
            [
                sys.executable,
                str(MODULE_PATH),
                "--root",
                str(self.root),
                "--receipt",
                str(receipt or self.receipt),
                "--cargo",
                str(self.cargo),
                "--rustfmt",
                str(rustfmt or self.rustfmt),
                "--rustc",
                str(self.rustc),
                "--candidate-sha",
                self.candidate_sha,
                "--candidate-tree-sha",
                self.tree_sha,
                "--timeout-seconds",
                str(timeout),
                "--max-output-bytes",
                str(max_output),
                "--max-findings",
                str(max_findings),
            ],
            text=True,
            capture_output=True,
            env=env,
            check=False,
        )

    def read_receipt(self, path: Path | None = None) -> dict[str, object]:
        return json.loads((path or self.receipt).read_text(encoding="utf-8"))

    def test_clean_workspace_passes_and_records_integration_target(self) -> None:
        result = self.run_check()
        self.assertEqual(result.returncode, 0, result.stderr)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "pass")
        self.assertEqual(receipt["workspace"]["manifest_count"], 2)
        sources = [target["source"] for target in receipt["workspace"]["targets"]]
        self.assertIn("xtask/tests/format_me.rs", sources)
        self.assertEqual([row["status"] for row in receipt["runs"]], ["pass", "pass"])

    def test_unformatted_library_file_is_a_format_failure(self) -> None:
        self.set_fmt(
            {
                "pkg-a/Cargo.toml": {
                    "exit": 1,
                    "stdout": f"Diff in {self.root / 'pkg-a/src/lib.rs'}:1:\n-old\n+new",
                }
            }
        )
        result = self.run_check()
        self.assertEqual(result.returncode, 1, result.stderr)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "format_failure")
        self.assertEqual(receipt["findings"][0]["path"], "pkg-a/src/lib.rs")
        self.assertEqual(receipt["findings"][0]["line"], 1)

    def test_unformatted_xtask_integration_test_is_in_scope(self) -> None:
        self.set_fmt(
            {
                "xtask/Cargo.toml": {
                    "exit": 1,
                    "stderr": f"Diff in {self.root / 'xtask/tests/format_me.rs'}:2:\n-old\n+new",
                }
            }
        )
        result = self.run_check()
        self.assertEqual(result.returncode, 1, result.stderr)
        receipt = self.read_receipt()
        self.assertEqual(receipt["findings"][0]["path"], "xtask/tests/format_me.rs")

    @unittest.skipUnless(os.name == "nt", "Windows extended path syntax")
    def test_windows_extended_diff_path_is_repository_contained(self) -> None:
        source = self.root / "pkg-a/src/lib.rs"
        extended_source = "\\\\?\\" + str(source)
        self.set_fmt(
            {
                "pkg-a/Cargo.toml": {
                    "exit": 1,
                    "stdout": f"Diff in {extended_source}:1:\n-old\n+new",
                }
            }
        )
        result = self.run_check()
        self.assertEqual(result.returncode, 1, result.stderr)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "format_failure")
        self.assertEqual(receipt["findings"][0]["path"], "pkg-a/src/lib.rs")

    def test_multiple_package_failures_survive_one_receipt(self) -> None:
        self.set_fmt(
            {
                "pkg-a/Cargo.toml": {
                    "exit": 1,
                    "stdout": f"Diff in {self.root / 'pkg-a/src/lib.rs'}:1:\n-a\n+b",
                },
                "xtask/Cargo.toml": {
                    "exit": 1,
                    "stdout": f"Diff in {self.root / 'xtask/src/main.rs'}:1:\n-a\n+b",
                },
            }
        )
        result = self.run_check()
        self.assertEqual(result.returncode, 1, result.stderr)
        receipt = self.read_receipt()
        self.assertEqual(len(receipt["findings"]), 2)
        self.assertEqual([row["status"] for row in receipt["runs"]], [
            "format_failure",
            "format_failure",
        ])

    def test_later_manifests_run_after_an_earlier_format_failure(self) -> None:
        self.set_fmt(
            {
                "pkg-a/Cargo.toml": {
                    "exit": 1,
                    "stdout": f"Diff in {self.root / 'pkg-a/src/lib.rs'}:1:\n-a\n+b",
                }
            }
        )
        result = self.run_check()
        self.assertEqual(result.returncode, 1, result.stderr)
        receipt = self.read_receipt()
        self.assertEqual([row["status"] for row in receipt["runs"]], [
            "format_failure",
            "pass",
        ])

    def test_metadata_failure_is_instrument_failure(self) -> None:
        result = self.run_check(extra_env={"FAKE_METADATA_FAIL": "1"})
        self.assertEqual(result.returncode, 2)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "instrument_failure")
        self.assertIn("metadata", receipt["reason"])

    def test_missing_rustfmt_is_instrument_failure(self) -> None:
        result = self.run_check(rustfmt=self.root / "missing-rustfmt")
        self.assertEqual(result.returncode, 2)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "instrument_failure")
        self.assertIn("could not start", receipt["reason"])

    def test_rustfmt_probe_failure_is_instrument_failure(self) -> None:
        result = self.run_check(extra_env={"FAKE_RUSTFMT_FAIL": "1"})
        self.assertEqual(result.returncode, 2)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "instrument_failure")
        self.assertIn("rustfmt version probe exited 7", receipt["reason"])

    def test_rustfmt_probe_timeout_is_instrument_failure(self) -> None:
        result = self.run_check(timeout=2.0, extra_env={"FAKE_RUSTFMT_SLEEP": "10"})
        self.assertEqual(result.returncode, 2)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "instrument_failure")
        self.assertIn("rustfmt version probe timed out", receipt["reason"])

    def test_formatter_timeout_is_instrument_failure(self) -> None:
        self.set_fmt({"pkg-a/Cargo.toml": {"sleep": 10}})
        result = self.run_check(timeout=3.0)
        self.assertEqual(result.returncode, 2)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "instrument_failure")
        self.assertTrue(any(row["timed_out"] for row in receipt["runs"]))

    def test_non_diff_failure_is_not_misclassified_as_format_failure(self) -> None:
        self.set_fmt({"pkg-a/Cargo.toml": {"exit": 1, "stderr": "rustfmt crashed"}})
        result = self.run_check()
        self.assertEqual(result.returncode, 2)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "instrument_failure")
        self.assertEqual(receipt["findings"], [])

    def test_mixed_diff_and_formatter_error_is_instrument_failure(self) -> None:
        self.set_fmt(
            {
                "pkg-a/Cargo.toml": {
                    "exit": 1,
                    "stdout": f"Diff in {self.root / 'pkg-a/src/lib.rs'}:1:\n-old\n+new",
                    "stderr": "error: this file contains an unclosed delimiter",
                }
            }
        )
        result = self.run_check()
        self.assertEqual(result.returncode, 2, result.stderr)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "instrument_failure")
        self.assertEqual(receipt["findings"], [])

    def test_cargo_uses_the_selected_rustfmt(self) -> None:
        result = self.run_check(extra_env={"EXPECT_RUSTFMT": str(self.rustfmt)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.read_receipt()["result"], "pass")

    def test_escaping_manifest_is_not_proven(self) -> None:
        outside = self.control / "outside.toml"
        outside.write_text("[package]\nname='outside'\nversion='0.1.0'\n", encoding="utf-8")
        self._write_metadata(manifest_override=str(outside))
        result = self.run_check()
        self.assertEqual(result.returncode, 2)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "not_proven")
        self.assertIn("escapes", receipt["reason"])

    def test_symlinked_manifest_is_not_proven(self) -> None:
        link = self.root / "linked-manifest.toml"
        try:
            link.symlink_to(self.root / "pkg-a/Cargo.toml")
        except OSError as error:
            self.skipTest(f"symbolic links unavailable: {error}")
        self._write_metadata(manifest_override=str(link))
        result = self.run_check()
        self.assertEqual(result.returncode, 2, result.stderr)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "not_proven")
        self.assertIn("symbolic link", receipt["reason"])

    @unittest.skipUnless(os.name == "nt", "Windows extended path syntax")
    def test_windows_extended_manifest_path_is_repository_contained(self) -> None:
        manifest = "\\\\?\\" + str(self.root / "pkg-a/Cargo.toml")
        self._write_metadata(manifest_override=manifest)
        result = self.run_check()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.read_receipt()["result"], "pass")

    def test_supplied_candidate_must_match_checked_out_identity(self) -> None:
        self.candidate_sha = "a" * 40
        result = self.run_check()
        self.assertEqual(result.returncode, 2)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "not_proven")
        self.assertIn("does not match", receipt["reason"])

    def test_dirty_candidate_is_not_proven(self) -> None:
        self._write("pkg-a/src/lib.rs", "pub fn value() -> u8 { 2 }\n")
        result = self.run_check()
        self.assertEqual(result.returncode, 2)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "not_proven")
        self.assertIn("not clean", receipt["reason"])

    def test_candidate_mutation_during_run_is_instrument_failure(self) -> None:
        mutation = self.root / "pkg-a/src/lib.rs"
        fake_cargo = self.cargo.with_suffix(".py") if os.name == "nt" else self.cargo
        script = fake_cargo.read_text(encoding="utf-8")
        script = script.replace(
            "time.sleep(float(result.get(\"sleep\", 0)))",
            "\n".join(
                [
                    "time.sleep(float(result.get(\"sleep\", 0)))",
                    f"    pathlib.Path({str(mutation)!r}).write_text('mutated\\n', encoding='utf-8')",
                ]
            ),
        )
        fake_cargo.write_text(script, encoding="utf-8")
        result = self.run_check()
        self.assertEqual(result.returncode, 2, result.stderr)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "instrument_failure")
        self.assertTrue(
            any("changed while" in row["reason"] for row in receipt["instrument_failures"]),
            receipt,
        )

    def test_clean_candidate_checkout_during_run_is_instrument_failure(self) -> None:
        original_sha = self.candidate_sha
        self._write("pkg-a/src/lib.rs", "pub fn value() -> u8 { 2 }\n")
        self._git("add", "pkg-a/src/lib.rs")
        self._git("commit", "-m", "alternate candidate")
        alternate_sha = self._git("rev-parse", "HEAD").stdout.strip()
        self._git("checkout", "--detach", original_sha)
        self.set_fmt({"pkg-a/Cargo.toml": {"checkout": alternate_sha}})

        result = self.run_check()

        self.assertEqual(result.returncode, 2, result.stderr)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "instrument_failure")
        self.assertTrue(
            any("commit or tree changed" in row["reason"] for row in receipt["instrument_failures"]),
            receipt,
        )

    def test_malformed_workspace_member_identity_is_not_proven(self) -> None:
        payload = json.loads(self.metadata.read_text(encoding="utf-8"))
        payload["workspace_members"] = [["not", "hashable"]]
        self.metadata.write_text(json.dumps(payload), encoding="utf-8")
        result = self.run_check()
        self.assertEqual(result.returncode, 2, result.stderr)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "not_proven")
        self.assertIn("workspace_members", receipt["reason"])

    def test_duplicate_workspace_package_record_is_not_proven(self) -> None:
        payload = json.loads(self.metadata.read_text(encoding="utf-8"))
        payload["packages"].append(payload["packages"][0])
        self.metadata.write_text(json.dumps(payload), encoding="utf-8")
        result = self.run_check()
        self.assertEqual(result.returncode, 2, result.stderr)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "not_proven")
        self.assertIn("duplicates workspace package", receipt["reason"])

    def test_output_overflow_is_instrument_failure(self) -> None:
        self.set_fmt({"pkg-a/Cargo.toml": {"exit": 1, "stdout": "x" * 100000}})
        result = self.run_check(max_output=4096)
        self.assertEqual(result.returncode, 2)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "instrument_failure")
        self.assertTrue(receipt["runs"][0]["stdout_truncated"])

    def test_findings_limit_is_explicit_without_corrupting_later_runs(self) -> None:
        self.set_fmt(
            {
                "pkg-a/Cargo.toml": {
                    "exit": 1,
                    "stdout": f"Diff in {self.root / 'pkg-a/src/lib.rs'}:1:\n-a\n+b",
                },
                "xtask/Cargo.toml": {
                    "exit": 1,
                    "stdout": f"Diff in {self.root / 'xtask/src/main.rs'}:1:\n-a\n+b",
                },
            }
        )
        result = self.run_check(max_findings=1)
        self.assertEqual(result.returncode, 2, result.stderr)
        receipt = self.read_receipt()
        self.assertEqual(receipt["result"], "instrument_failure")
        self.assertTrue(receipt["findings_truncated"])
        self.assertEqual(len(receipt["findings"]), 1)
        self.assertEqual(
            [row["status"] for row in receipt["runs"]],
            ["format_failure", "format_failure"],
        )
        self.assertEqual(len(receipt["instrument_failures"]), 1)

    def test_receipt_write_failure_returns_instrument_error(self) -> None:
        blocker = self.control / "blocker"
        blocker.write_text("not a directory", encoding="utf-8")
        result = self.run_check(receipt=blocker / "receipt.json")
        self.assertEqual(result.returncode, 2)
        self.assertIn("could not persist receipt", result.stderr)

    def test_fixed_inputs_produce_byte_identical_receipts(self) -> None:
        first = self.root / "target/first.json"
        second = self.root / "target/second.json"
        first_result = self.run_check(receipt=first)
        second_result = self.run_check(receipt=second)
        self.assertEqual(first_result.returncode, 0)
        self.assertEqual(second_result.returncode, 0)
        self.assertEqual(first.read_bytes(), second.read_bytes())


if __name__ == "__main__":
    unittest.main()
