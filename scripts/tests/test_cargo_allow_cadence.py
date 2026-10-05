"""CLI and refusal oracles for the optional upstream cadence capture (#15304)."""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "cargo_allow_cadence.py"
SPEC = importlib.util.spec_from_file_location("cargo_allow_cadence", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("capture module could not be loaded")
capture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(capture)

BACKEND = r'''
import argparse, json, os, subprocess, sys, time
from pathlib import Path
p = argparse.ArgumentParser()
p.add_argument("command")
for key in ("root", "config", "as-of", "format"):
    p.add_argument("--" + key, required=True)
a = p.parse_args()
root = Path(a.root)
(root / "child-marker").write_text("started")
mode = (root / "mode").read_text()
report = {
 "schema_id": "cargo-allow.cadence.v1", "schema_version": 1,
 "tool": "cargo-allow", "command": "cadence", "as_of": a.as_of,
 "as_of_source": "explicit", "policy_path": a.config,
 "horizons": {"review_due_soon_days": 14, "expiring_soon_days": 14},
 "inventory": {"root": a.root, "scope": "source_tree", "scanner": "source_syntax", "source": "git_tracked", "files_scanned": 1, "empty_git_tracked": False, "completeness": "scoped"},
 "rows": [{"allow_id": "boundary-row", "class": "expiring", "owner": "unowned", "classification": "baseline_debt", "source_path": None, "source_glob": "src/**", "selector_summary": "panic", "evidence_refs": ["fixture"], "review_after": None, "expires": "2026-10-20", "days_remaining": 0, "driving_date": "expires", "required_disposition": "renew, narrow, or plan removal"}],
 "summary": {"total_entries": 1, "current": 0, "review_due_soon": 0, "review_overdue": 0, "expiring": 1, "expired": 0, "invalid": 0}, "claim_boundary": ["source_tree_inventory"],
 "scanner_limitations": ["repository_code_not_executed"]
}
if mode == "wrong-date": report["as_of"] = "2026-10-21"
if mode == "ambient-date": report["as_of_source"] = "system_clock"
if mode == "wrong-policy": report["policy_path"] += ".wrong"
if mode == "wrong-root": report["inventory"]["root"] += ".wrong"
if mode == "wrong-schema": report["schema_id"] = "cargo-allow.check.v1"
if mode == "boolean-version": report["schema_version"] = True
if mode == "wrong-horizons": report["horizons"]["expiring_soon_days"] = 7
if mode == "partial-row": report["rows"] = [{}]
if mode == "wrong-summary": report["summary"]["total_entries"] = 0
if mode == "malformed-class": report["rows"][0]["class"] = []
if mode == "warning": sys.stderr.buffer.write(b"upstream warning\xff\n")
if mode == "malformed": print("{"); sys.exit(0)
if mode == "duplicate": print('{"schema_id":"cargo-allow.cadence.v1","schema_id":"cargo-allow.cadence.v1"}'); sys.exit(0)
if mode == "stdout-cap": sys.stdout.write("x" * (1024 * 1024 + 1)); sys.exit(0)
if mode == "stderr-cap": sys.stderr.write("x" * (64 * 1024 + 1)); sys.exit(0)
if mode == "fail": print("partial"); print("failed", file=sys.stderr); sys.exit(7)
if mode == "timeout": print("partial", flush=True); time.sleep(10)
if mode == "descendant":
 child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(10)"])
 (root / "descendant-pid").write_text(str(child.pid))
 sys.exit(0)
if mode == "change-policy": Path(a.config).write_text("changed policy")
if mode == "change-executable":
 with open(__file__, "a") as executable: executable.write("\n# changed executable\n")
raw = " \n" + json.dumps(report, indent=1) + "\n\n"
(root / "expected-stdout").write_bytes(raw.encode())
sys.stdout.buffer.write(raw.encode())
'''


class AdmissionTests(unittest.TestCase):
    def test_explicit_date_and_absolute_paths(self):
        for date in ("today", "2026-2-03", "2026-02-30"):
            with self.assertRaises(capture.CaptureError):
                capture.explicit_date(date)
        self.assertEqual(capture.explicit_date("2026-10-20"), "2026-10-20")
        with self.assertRaises(capture.CaptureError):
            capture.absolute_path("relative", "--root")


@unittest.skipUnless(os.name == "posix", "real process capture uses POSIX/WSL")
class CaptureTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="cadence proof ")
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()
        self.root = self.base / "consumer root"
        (self.root / "policy").mkdir(parents=True)
        self.policy = self.root / "policy" / "allow.toml"
        self.policy.write_text("# pinned policy fixture\n")
        self.executable = self.base / "fake cargo allow"
        self.executable.write_text(f"#!{sys.executable}\n" + BACKEND)
        self.executable.chmod(0o700)
        self.digest = hashlib.sha256(self.executable.read_bytes()).hexdigest()
        self.output = self.base / "captured result"
        self.mode("valid")

    def mode(self, value):
        (self.root / "mode").write_text(value)

    def args(self, **overrides):
        values = dict(executable=str(self.executable), expected_sha256=self.digest,
                      root=str(self.root), output_dir=str(self.output), as_of="2026-10-20")
        values.update(overrides)
        return argparse.Namespace(**values)

    def cli(self, **overrides):
        argv = [sys.executable, str(SCRIPT)]
        for key, value in vars(self.args(**overrides)).items():
            argv.extend(["--" + key.replace("_", "-"), value])
        return subprocess.run(argv, cwd=self.base, capture_output=True, timeout=5)

    def sentinels(self):
        self.output.mkdir()
        (self.output / capture.REPORT_NAME).write_bytes(b"existing report")
        (self.output / capture.RECEIPT_NAME).write_bytes(b"existing receipt")
        (self.output / capture.STDERR_NAME).write_bytes(b"existing stderr")

    def assert_sentinels(self):
        self.assertEqual((self.output / capture.REPORT_NAME).read_bytes(), b"existing report")
        self.assertEqual((self.output / capture.RECEIPT_NAME).read_bytes(), b"existing receipt")
        self.assertEqual((self.output / capture.STDERR_NAME).read_bytes(), b"existing stderr")

    def test_wrong_digest_refuses_before_child_then_correct_digest_runs(self):
        self.sentinels()
        result = self.cli(expected_sha256="0" * 64)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"child was not started", result.stderr)
        self.assertFalse((self.root / "child-marker").exists())
        self.assert_sentinels()
        result = self.cli(output_dir=str(self.base / "positive control"))
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertTrue((self.root / "child-marker").exists())

    def test_cli_preserves_raw_report_and_14_day_semantics(self):
        result = self.cli()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        raw = (self.output / capture.REPORT_NAME).read_bytes()
        self.assertEqual(raw, (self.root / "expected-stdout").read_bytes())
        report = json.loads(raw)
        self.assertEqual(report["horizons"], {"review_due_soon_days": 14, "expiring_soon_days": 14})
        self.assertEqual(report["rows"][0]["class"], "expiring")
        receipt = json.loads((self.output / capture.RECEIPT_NAME).read_bytes())
        self.assertTrue(receipt["advisory_only"])
        self.assertEqual(receipt["report_sha256"], hashlib.sha256(raw).hexdigest())
        self.assertEqual(receipt["policy_sha256"], {"before": capture.sha256_file(self.policy), "after": capture.sha256_file(self.policy)})
        self.assertIn("no_whole_worktree_snapshot", receipt["claim_boundary"])

    def test_identical_repeat_keeps_bytes_and_mtimes(self):
        self.assertEqual(self.cli().returncode, 0)
        before = {path.name: (path.read_bytes(), path.stat().st_mtime_ns) for path in self.output.iterdir()}
        result = self.cli()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertIn(b"unchanged", result.stdout)
        self.assertEqual(before, {path.name: (path.read_bytes(), path.stat().st_mtime_ns) for path in self.output.iterdir()})

    def test_bad_reports_and_child_failure_preserve_existing_outputs(self):
        self.sentinels()
        cases = {"wrong-date": b"as_of", "ambient-date": b"as_of_source", "wrong-policy": b"policy_path", "wrong-root": b"inventory.root", "wrong-schema": b"schema_id", "boolean-version": b"schema_version", "wrong-horizons": b"horizons", "malformed": b"complete UTF-8 JSON", "duplicate": b"duplicate upstream JSON key", "partial-row": b"row allow_id", "wrong-summary": b"summary does not match", "malformed-class": b"row class is invalid", "fail": b"exited 7"}
        for mode, diagnostic in cases.items():
            with self.subTest(mode=mode):
                self.mode(mode)
                result = self.cli()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(diagnostic, result.stderr)
                self.assert_sentinels()

    def test_successful_stderr_is_preserved_and_bound(self):
        self.mode("warning")
        result = self.cli()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        stderr = (self.output / capture.STDERR_NAME).read_bytes()
        self.assertEqual(stderr, b"upstream warning\xff\n")
        receipt = json.loads((self.output / capture.RECEIPT_NAME).read_bytes())
        self.assertEqual(receipt["stderr_sha256"], hashlib.sha256(stderr).hexdigest())
        self.assertEqual(receipt["stderr_bytes"], len(stderr))

    def test_changed_policy_and_executable_refuse_publication(self):
        self.sentinels()
        for mode in ("change-policy", "change-executable"):
            with self.subTest(mode=mode):
                self.mode(mode)
                result = self.cli()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b"changed during capture", result.stderr)
                self.assert_sentinels()

    def test_each_stream_is_bounded_and_failure_preserves_outputs(self):
        self.sentinels()
        for mode in ("stdout-cap", "stderr-cap"):
            with self.subTest(mode=mode):
                self.mode(mode)
                result = self.cli()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b"byte bound", result.stderr)
                self.assert_sentinels()

    def test_timeout_and_descendant_pipe_are_bounded(self):
        self.sentinels()
        for mode in ("timeout", "descendant"):
            with self.subTest(mode=mode):
                self.mode(mode)
                with patch.object(capture, "TIMEOUT_SECONDS", 0.2):
                    with self.assertRaisesRegex(capture.CaptureError, "time bound"):
                        capture.capture(self.args())
                self.assert_sentinels()

    def test_output_symlink_and_hardlink_aliases_refuse_before_child(self):
        self.output.mkdir()
        receipt = self.output / capture.RECEIPT_NAME
        receipt.write_bytes(b"existing receipt")
        (self.output / capture.STDERR_NAME).write_bytes(b"existing stderr")
        artifact = self.output / capture.REPORT_NAME
        for source in (self.policy, self.executable):
            for kind in ("symlink", "hardlink"):
                with self.subTest(source=source, kind=kind):
                    before = source.read_bytes()
                    if kind == "symlink": artifact.symlink_to(source)
                    else: os.link(source, artifact)
                    try:
                        result = self.cli()
                        self.assertNotEqual(result.returncode, 0)
                        self.assertFalse((self.root / "child-marker").exists())
                        self.assertEqual(source.read_bytes(), before)
                        self.assertEqual(receipt.read_bytes(), b"existing receipt")
                    finally:
                        artifact.unlink()

    def test_different_prior_capture_is_not_overwritten(self):
        self.sentinels()
        result = self.cli()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"choose a fresh", result.stderr)
        self.assert_sentinels()

    def test_publication_failure_leaves_no_partial_output(self):
        with patch.object(Path, "rename", side_effect=OSError("publication denied")):
            with self.assertRaisesRegex(OSError, "publication denied"):
                capture.capture(self.args())
        self.assertFalse(self.output.exists())
        self.assertEqual(list(self.base.glob(".cargo-allow-cadence-*")), [])


if __name__ == "__main__":
    unittest.main()
