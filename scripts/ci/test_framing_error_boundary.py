"""Tests of the probe's decision logic; these do not execute Rust."""
from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

_SPEC = importlib.util.spec_from_file_location(
    "framing_boundary", Path(__file__).with_name("check_framing_error_boundary.py")
)
if _SPEC is None or _SPEC.loader is None:
    raise RuntimeError("cannot load framing boundary probe")
probe = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(probe)


def diagnostic(code="E0432", message="unresolved import `perl_parser_core`"):
    return json.dumps({"level": "error", "code": {"code": code}, "message": message})


def output(code=1, stderr=""):
    return subprocess.CompletedProcess(["rustc"], code, stdout="", stderr=stderr)


class TaxonomyRejectionTests(unittest.TestCase):
    def test_direct_and_aliased_dependency_errors_are_discriminated(self):
        for code in ["E0432", "E0433"]:
            self.assertTrue(probe.taxonomy_rejection(output(stderr=diagnostic(code))))

    def test_success_never_satisfies_a_negative_control(self):
        self.assertFalse(probe.taxonomy_rejection(output(0, diagnostic())))

    def test_compiler_crash_is_not_a_passing_negative_control(self):
        for code in [-11, 2, 101, 127]:
            self.assertFalse(probe.taxonomy_rejection(output(code, diagnostic())))

    def test_unrelated_error_or_dependency_does_not_count(self):
        for error in [diagnostic("E0308"), diagnostic(message="unresolved import `other_crate`")]:
            self.assertFalse(probe.taxonomy_rejection(output(stderr=error)))

    def test_taxonomy_error_cannot_hide_an_unrelated_error(self):
        stderr = diagnostic() + "\n" + diagnostic("E0308", "mismatched types")
        self.assertFalse(probe.taxonomy_rejection(output(stderr=stderr)))

    def test_no_diagnostic_or_malformed_diagnostic_is_not_evidence(self):
        for stderr in ["", "permission denied", "{broken", "null", "[]"]:
            self.assertFalse(probe.taxonomy_rejection(output(stderr=stderr)))

    def test_only_rustc_abort_summary_can_be_uncoded(self):
        summary = json.dumps({"level": "error", "code": None, "message": "aborting due to 1 previous error"})
        self.assertTrue(probe.taxonomy_rejection(output(stderr=diagnostic() + "\n" + summary)))
        syntax = json.dumps({"level": "error", "code": None, "message": "expected item"})
        self.assertFalse(probe.taxonomy_rejection(output(stderr=diagnostic() + "\n" + syntax)))

    def test_source_path_quoting_preserves_windows_quotes_and_unicode(self):
        path = 'C:\\code with spaces\\"quoted"\\café.rs'
        self.assertEqual(json.loads(probe.rust_string(path)), path)

    def test_control_characters_in_paths_are_rejected(self):
        for character in ["\n", "\r", "\t", "\x00"]:
            with self.assertRaises(ValueError):
                probe.rust_string("root" + character + "file.rs")


class ExecutionRoutingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="framing-probe-unit-")
        self.root = Path(self.temporary.name)
        self.source = self.root / probe.SOURCE
        self.source.parent.mkdir(parents=True)
        self.source.write_text("pub enum FramingError {}\n", encoding="utf-8")
        self.addCleanup(self.temporary.cleanup)

    def test_missing_compiler_is_instrument_failure(self):
        with patch.object(probe.shutil, "which", return_value=None):
            code, receipt = probe.execute(self.root)
        self.assertEqual(code, 2)
        self.assertEqual(receipt["outcome"], "instrument_failure")
        self.assertEqual(receipt["cases"], [])

    def test_rustup_cannot_install_tools_and_output_is_metadata_only(self):
        version = subprocess.CompletedProcess(["rustc"], 0, "rustc test-double", "")
        with patch.object(probe.shutil, "which", return_value="/toolchain/rustc"), patch.object(
            probe.subprocess, "run", side_effect=[version, output(0), output(stderr=diagnostic()), output(stderr=diagnostic())]
        ) as run:
            code, receipt = probe.execute(self.root)
        self.assertEqual(code, 0)
        self.assertEqual(len(receipt["cases"]), 3)
        for call in run.call_args_list:
            self.assertEqual(call.kwargs["env"]["RUSTUP_AUTO_INSTALL"], "0")
            self.assertEqual(call.kwargs["cwd"], self.root.resolve())
        for call in run.call_args_list[1:]:
            self.assertIn("--emit=metadata", call.args[0])
            self.assertNotIn("--extern", call.args[0])

    def test_positive_compile_failure_stops_before_negative_controls(self):
        version = subprocess.CompletedProcess(["rustc"], 0, "rustc test-double", "")
        with patch.object(probe.shutil, "which", return_value="/toolchain/rustc"), patch.object(
            probe.subprocess, "run", side_effect=[version, output(stderr=diagnostic("E0308"))]
        ):
            code, receipt = probe.execute(self.root)
        self.assertEqual(code, 1)
        self.assertEqual(receipt["reason"], "neutral_failed")
        self.assertEqual(len(receipt["cases"]), 1)

    def test_changing_source_cannot_inherit_a_passing_compiler_result(self):
        version = subprocess.CompletedProcess(["rustc"], 0, "rustc test-double", "")
        calls = 0
        def changing_compiler(*args, **kwargs):
            nonlocal calls
            calls += 1
            if calls == 1:
                return version
            self.source.write_text("pub struct FramingError;\n", encoding="utf-8")
            return output(0)
        with patch.object(probe.shutil, "which", return_value="/toolchain/rustc"), patch.object(
            probe.subprocess, "run", side_effect=changing_compiler
        ):
            code, receipt = probe.execute(self.root)
        self.assertEqual(code, 2)
        self.assertEqual(receipt["reason"], "source_changed_during_probe")

    def test_negative_control_success_is_a_rejection(self):
        version = subprocess.CompletedProcess(["rustc"], 0, "rustc test-double", "")
        with patch.object(probe.shutil, "which", return_value="/toolchain/rustc"), patch.object(
            probe.subprocess, "run", side_effect=[version, output(0), output(0)]
        ):
            code, receipt = probe.execute(self.root)
        self.assertEqual(code, 1)
        self.assertEqual(receipt["reason"], "direct_taxonomy_failed")


if __name__ == "__main__":
    unittest.main()
