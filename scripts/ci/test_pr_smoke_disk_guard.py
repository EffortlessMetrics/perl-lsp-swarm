#!/usr/bin/env python3
"""Focused tests for scripts/ci/pr_smoke_disk_guard.py (#11943).

The disk-exhaustion failure class is proven here at the logic layer: the
preflight budget decision, the exhaustion-signature classification, and the
pressure-log composition are all exercised against synthetic inputs so no
multi-gigabyte local build is needed to falsify a regression.
"""

from __future__ import annotations

import argparse
import importlib.util
import io
import json
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

SCRIPT = Path(__file__).with_name("pr_smoke_disk_guard.py")
SPEC = importlib.util.spec_from_file_location("pr_smoke_disk_guard", SCRIPT)
assert SPEC and SPEC.loader
guard = importlib.util.module_from_spec(SPEC)
# Dataclass resolution inspects sys.modules under the module's own name.
sys.modules["pr_smoke_disk_guard"] = guard
SPEC.loader.exec_module(guard)


class FakeUsage:
    def __init__(self, total: int, used: int, free: int) -> None:
        self.total = total
        self.used = used
        self.free = free


def fake_probe(table: dict[str, FakeUsage]):
    def probe(path: str) -> FakeUsage:
        try:
            return table[path]
        except KeyError as error:
            raise FileNotFoundError(f"no such path: {path}") from error

    return probe


class FootprintFloorTests(unittest.TestCase):
    """The floor must exceed the declared footprint (#17231)."""

    def test_default_floor_exceeds_measured_footprint(self) -> None:
        self.assertGreater(
            guard.effective_min_free_bytes(None),
            guard.MEASURED_FOOTPRINT_BYTES,
        )

    def test_floor_is_footprint_plus_reserve(self) -> None:
        self.assertEqual(
            guard.effective_min_free_bytes(None),
            guard.MEASURED_FOOTPRINT_BYTES + guard.RESERVE_BYTES,
        )

    def test_override_at_or_below_footprint_is_config_error(self) -> None:
        with self.assertRaises(ValueError):
            guard.effective_min_free_bytes(guard.MEASURED_FOOTPRINT_BYTES)
        with self.assertRaises(ValueError):
            guard.effective_min_free_bytes(32 * guard.GIB)

    def test_override_above_footprint_is_honoured(self) -> None:
        self.assertEqual(
            guard.effective_min_free_bytes(120 * guard.GIB), 120 * guard.GIB
        )

    def test_runner_with_footprint_but_no_reserve_is_rejected(self) -> None:
        # 86 GiB free (fresh hosted runner) vs 85 GiB footprint + reserve:
        # the old 32 GiB floor accepted this and the lane died ENOSPC.
        table = {".": FakeUsage(145 * guard.GIB, 59 * guard.GIB, 86 * guard.GIB)}
        breaches = guard.evaluate_preflight(
            {".": "."}, guard.effective_min_free_bytes(None), probe=fake_probe(table)
        )
        self.assertEqual(len(breaches), 1)

    def test_runner_covering_footprint_plus_reserve_is_accepted(self) -> None:
        table = {".": FakeUsage(200 * guard.GIB, 80 * guard.GIB, 120 * guard.GIB)}
        breaches = guard.evaluate_preflight(
            {".": "."}, guard.effective_min_free_bytes(None), probe=fake_probe(table)
        )
        self.assertEqual(breaches, [])


class FootprintBaselineTests(unittest.TestCase):
    """The committed baseline JSON is the guard's single source (#19067)."""

    def test_committed_baseline_matches_guard_constants(self) -> None:
        baseline = Path(__file__).resolve().parents[2] / ".ci" / "pr-smoke-footprint-baseline.json"
        self.assertTrue(baseline.is_file())
        footprint, reserve = guard.load_footprint_baseline(baseline)
        self.assertEqual(footprint, 85 * guard.GIB)
        self.assertEqual(reserve, 5 * guard.GIB)
        self.assertEqual(guard.MEASURED_FOOTPRINT_BYTES, footprint)
        self.assertEqual(guard.RESERVE_BYTES, reserve)
        self.assertEqual(guard.effective_min_free_bytes(None), 90 * guard.GIB)

    def test_committed_baseline_records_expired_provenance(self) -> None:
        baseline = Path(__file__).resolve().parents[2] / ".ci" / "pr-smoke-footprint-baseline.json"
        data = json.loads(baseline.read_text(encoding="utf-8"))
        self.assertEqual(data["footprint_gib"], 85)
        self.assertEqual(data["reserve_gib"], 5)
        self.assertEqual(data["source_run_id"], 37195503421)
        self.assertEqual(data["provenance_status"], "expired-unverifiable")
        self.assertEqual(data["recorded_date"], "2026-11-08")
        self.assertEqual(data["runner"], "ubuntu-24.04")

    def test_baseline_json_overrides_fallback_when_present(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "baseline.json"
            path.write_text(
                '{"footprint_gib": 100, "reserve_gib": 10}',
                encoding="utf-8",
            )
            self.assertEqual(
                guard.load_footprint_baseline(path),
                (100 * guard.GIB, 10 * guard.GIB),
            )

    def test_low_baseline_never_lowers_the_floor(self) -> None:
        # A well-formed baseline below the in-code fallback must not
        # lower the floor on its own; only a reviewed change that also
        # lowers the fallback (with a fresh measurement) may do that.
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "baseline.json"
            path.write_text(
                '{"footprint_gib": 1, "reserve_gib": 0}',
                encoding="utf-8",
            )
            self.assertEqual(
                guard.load_footprint_baseline(path),
                (85 * guard.GIB, 5 * guard.GIB),
            )

    def test_missing_baseline_falls_back_to_in_code_constants(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            missing = Path(tmp) / "absent.json"
            self.assertEqual(
                guard.load_footprint_baseline(missing),
                (85 * guard.GIB, 5 * guard.GIB),
            )

    def test_malformed_baseline_falls_back_without_lowering_floor(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            for text in ("not json", '{"footprint_gib": 1}', '{"footprint_gib": "85", "reserve_gib": 5}'):
                path = Path(tmp) / "baseline.json"
                path.write_text(text, encoding="utf-8")
                with self.subTest(text=text):
                    self.assertEqual(
                        guard.load_footprint_baseline(path),
                        (85 * guard.GIB, 5 * guard.GIB),
                    )


class ParseMinFreeBytesTests(unittest.TestCase):
    def test_plain_bytes(self) -> None:
        self.assertEqual(guard.parse_min_free_bytes("4096"), 4096)

    def test_gib_suffix(self) -> None:
        self.assertEqual(guard.parse_min_free_bytes("32GiB"), 32 * guard.GIB)

    def test_gib_suffix_case_insensitive(self) -> None:
        self.assertEqual(guard.parse_min_free_bytes("2gib"), 2 * guard.GIB)

    def test_finite_gib_controls(self) -> None:
        for raw, expected in (("0GiB", 0), ("0.5GiB", guard.GIB // 2), ("1e2GiB", 100 * guard.GIB)):
            with self.subTest(raw=raw):
                self.assertEqual(guard.parse_min_free_bytes(raw), expected)

    def test_rejects_non_finite_gib_and_byte_conversion_overflow(self) -> None:
        for raw in ("infGiB", "-infGiB", "NaNGiB", "1e309GiB", "1e308GiB"):
            with self.subTest(raw=raw):
                with self.assertRaises(argparse.ArgumentTypeError):
                    guard.parse_min_free_bytes(raw)

    def test_large_gib_value_preserves_integer_precision(self) -> None:
        self.assertEqual(
            guard.parse_min_free_bytes("9007199254740993GiB"),
            9007199254740993 * guard.GIB,
        )

    def test_fractional_gib_truncates_only_fractional_bytes(self) -> None:
        self.assertEqual(guard.parse_min_free_bytes("0.1GiB"), guard.GIB // 10)
        self.assertEqual(guard.parse_min_free_bytes("1e-10GiB"), 0)

    def test_extreme_exponents_are_bounded_without_huge_integers(self) -> None:
        for raw in ("1e999999999GiB", "-1e999999999GiB", "1e308GiB"):
            with self.subTest(raw=raw):
                with self.assertRaises(argparse.ArgumentTypeError):
                    guard.parse_min_free_bytes(raw)
        self.assertEqual(guard.parse_min_free_bytes("1e-999999999GiB"), 0)

    def test_zero_with_extreme_exponent_parses_to_zero(self) -> None:
        self.assertEqual(guard.parse_min_free_bytes("0e500GiB"), 0)
        self.assertEqual(guard.parse_min_free_bytes("-0e500GiB"), 0)

    def test_rejects_nonfinite_gib(self) -> None:
        for raw in ("NaNGiB", "sNaNGiB", "infGiB", "-InfinityGiB"):
            with self.subTest(raw=raw):
                with self.assertRaises(argparse.ArgumentTypeError):
                    guard.parse_min_free_bytes(raw)

    def test_cli_rejects_non_finite_budgets_without_traceback(self) -> None:
        for option in ("--footprint-bytes", "--min-free-bytes"):
            for raw in ("infGiB", "-infGiB", "NaNGiB", "1e309GiB", "1e308GiB"):
                with self.subTest(option=option, raw=raw):
                    result = subprocess.run(
                        [sys.executable, "-B", str(SCRIPT), "preflight", f"{option}={raw}"],
                        capture_output=True,
                        text=True,
                        check=False,
                    )
                    self.assertEqual(result.returncode, 2, result.stderr)
                    self.assertIn("must be finite", result.stderr)
                    self.assertNotIn("Traceback", result.stderr)

    def test_nonfinite_cli_arguments_fail_without_traceback(self) -> None:
        for flag in ("--min-free-bytes", "--footprint-bytes"):
            with self.subTest(flag=flag):
                result = subprocess.run(
                    [sys.executable, "-B", str(SCRIPT), "preflight", flag, "infGiB"],
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertEqual(result.returncode, 2)
                self.assertIn("finite", result.stderr)
                self.assertNotIn("Traceback", result.stderr)

    def test_rejects_garbage(self) -> None:
        with self.assertRaises(argparse.ArgumentTypeError):
            guard.parse_min_free_bytes("lots")

    def test_rejects_negative(self) -> None:
        with self.assertRaises(argparse.ArgumentTypeError):
            guard.parse_min_free_bytes("-1")


class EvaluatePreflightTests(unittest.TestCase):
    def test_breach_names_path_and_budget(self) -> None:
        table = {
            "/ws": FakeUsage(total=10000, used=10, free=9990),
            "/mnt/target": FakeUsage(total=200, used=190, free=10),
        }
        breaches = guard.evaluate_preflight(
            {"workspace": "/ws", "cargo-target-dir": "/mnt/target"},
            min_free_bytes=1024,
            probe=fake_probe(table),
        )
        self.assertEqual(len(breaches), 1)
        self.assertIn("/mnt/target", breaches[0])
        self.assertIn("cargo-target-dir", breaches[0])
        self.assertIn("10 bytes free", breaches[0])
        self.assertIn("1024 bytes", breaches[0])

    def test_unprobeable_path_fails_closed(self) -> None:
        def broken_probe(path: str) -> FakeUsage:
            raise OSError(f"cannot stat {path}")

        breaches = guard.evaluate_preflight(
            {"cargo-target-dir": "/mnt/gone"},
            min_free_bytes=1024,
            probe=broken_probe,
        )
        self.assertEqual(len(breaches), 1)
        self.assertIn("/mnt/gone", breaches[0])
        self.assertIn("Failing closed", breaches[0])

    def test_absent_target_dir_probes_nearest_existing_ancestor(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            absent_target = root / "not-created-yet" / "target"
            table = {str(root): FakeUsage(total=1000, used=10, free=990)}

            breaches = guard.evaluate_preflight(
                {"cargo-target-dir": str(absent_target)},
                min_free_bytes=512,
                probe=fake_probe(table),
            )
            self.assertEqual(breaches, [])

    def test_snapshot_marks_unprobeable_path_unavailable(self) -> None:
        def broken_probe(path: str) -> FakeUsage:
            raise OSError("boom")

        snapshot = guard.render_snapshot(
            {"cargo-target-dir": "/mnt/gone"}, probe=broken_probe
        )
        self.assertIn("unavailable", snapshot)
        self.assertIn("/mnt/gone", snapshot)

    def test_pass_when_every_path_has_headroom(self) -> None:
        table = {
            "/ws": FakeUsage(total=1000, used=10, free=990),
            "/mnt/target": FakeUsage(total=1000, used=10, free=990),
        }
        breaches = guard.evaluate_preflight(
            {"workspace": "/ws", "cargo-target-dir": "/mnt/target"},
            min_free_bytes=512,
            probe=fake_probe(table),
        )
        self.assertEqual(breaches, [])

    def test_each_breaching_path_reported(self) -> None:
        table = {
            "/ws": FakeUsage(total=100, used=99, free=1),
            "/mnt/target": FakeUsage(total=100, used=99, free=2),
        }
        breaches = guard.evaluate_preflight(
            {"workspace": "/ws", "cargo-target-dir": "/mnt/target"},
            min_free_bytes=1024,
            probe=fake_probe(table),
        )
        self.assertEqual(len(breaches), 2)


class ScanLogsTests(unittest.TestCase):
    def write_log(self, logs_dir: Path, name: str, text: str) -> Path:
        path = logs_dir / name
        path.write_text(text, encoding="utf-8")
        return path

    def test_detects_enospc_and_sigbus_with_class(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            logs_dir = Path(tmp)
            unit_log = self.write_log(
                logs_dir,
                "unit_routed_full.log",
                "collect2: fatal error: ld terminated with signal 7 [Bus error]",
            )
            clippy_log = self.write_log(
                logs_dir,
                "clippy_full.log",
                "error: No space left on device (os error 28)",
            )
            findings = guard.scan_logs(logs_dir)
            by_log = {finding.log_path: finding for finding in findings}
            self.assertEqual(
                by_log[str(unit_log)].signature_class, "link_sigbus"
            )
            self.assertIn("signal", by_log[str(unit_log)].signature_text)
            self.assertEqual(by_log[str(clippy_log)].signature_class, "enospc")

    def test_multi_signature_log_yields_single_strongest_finding(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            logs_dir = Path(tmp)
            # The observed run-32697324730 receipt contains all three failure
            # families at once; the log must contribute exactly one finding,
            # the first (strongest) match, so classify emits one annotation.
            self.write_log(
                logs_dir,
                "unit_routed_full.log",
                "collect2: fatal error: ld terminated with signal 7 [Bus error]\n"
                "rustc-LLVM ERROR: IO failure on output stream: No space left\n"
                "error: No space left on device (os error 28)",
            )
            findings = guard.scan_logs(logs_dir)
            self.assertEqual(len(findings), 1)
            self.assertEqual(findings[0].signature_class, "enospc")

    def test_other_kill_signals_do_not_fabricate_link_sigbus(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            logs_dir = Path(tmp)
            for signal in ("15", "9"):
                self.write_log(
                    logs_dir,
                    f"timeout_{signal}.log",
                    f"collect2: fatal error: ld terminated with signal {signal}",
                )
            self.assertEqual(guard.scan_logs(logs_dir), [])

    def test_clean_logs_produce_no_findings(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            logs_dir = Path(tmp)
            self.write_log(logs_dir, "fmt.log", "all checks passed")
            self.assertEqual(guard.scan_logs(logs_dir), [])

    def test_missing_logs_dir_is_tolerated(self) -> None:
        self.assertEqual(guard.scan_logs(Path("Z:/does/not/exist")), [])

    def test_non_log_files_ignored(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            logs_dir = Path(tmp)
            (logs_dir / "notes.txt").write_text(
                "No space left on device", encoding="utf-8"
            )
            self.assertEqual(guard.scan_logs(logs_dir), [])


class FindingAnnotationTests(unittest.TestCase):
    def test_annotation_names_log_target_and_pressure_log(self) -> None:
        finding = guard.Finding(
            log_path="/ws/target/receipts/logs/unit_routed_full.log",
            signature_class="enospc",
            signature_text="No space left on device",
        )
        annotation = finding.annotation("/mnt/perl-lsp-swarm/pr-smoke-1")
        self.assertIn(finding.log_path, annotation)
        self.assertIn("/mnt/perl-lsp-swarm/pr-smoke-1", annotation)
        self.assertIn("[enospc]", annotation)
        self.assertIn("pr-fast-disk-pressure.log", annotation)


class ClassifyFlowTests(unittest.TestCase):
    def run_classify(self, logs_dir: Path, pressure_log: Path, target: str) -> str:
        buffer = io.StringIO()
        with redirect_stdout(buffer):
            status = guard.run_classify(
                argparse.Namespace(
                    logs_dir=str(logs_dir),
                    target_dir=target,
                    pressure_log=str(pressure_log),
                )
            )
        self.assertEqual(status, 0)
        return buffer.getvalue()

    def test_finding_emits_error_annotation_and_appends_verdict(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            logs_dir = root / "logs"
            logs_dir.mkdir()
            (logs_dir / "unit_routed_full.log").write_text(
                "rustc-LLVM ERROR: IO failure on output stream: "
                "No space left on device",
                encoding="utf-8",
            )
            pressure_log = root / "pr-fast-disk-pressure.log"
            output = self.run_classify(logs_dir, pressure_log, "/mnt/target-x")
            self.assertIn("::error::resource-exhaustion", output)
            verdict = pressure_log.read_text(encoding="utf-8")
            self.assertIn("classification", verdict)
            self.assertIn("VERDICT: resource-exhaustion detected", verdict)
            self.assertIn("/mnt/target-x", verdict)

    def test_lone_llvm_stream_error_does_not_exonerate_candidate(self) -> None:
        # Negative control: without any ENOSPC / os-error-28 text in
        # the same run, a generic LLVM stream error must not produce the
        # definitive disk-exhaustion verdict.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            logs_dir = root / "logs"
            logs_dir.mkdir()
            (logs_dir / "unit_routed_full.log").write_text(
                "rustc-LLVM ERROR: IO failure on output stream: broken pipe",
                encoding="utf-8",
            )
            pressure_log = root / "pr-fast-disk-pressure.log"
            output = self.run_classify(logs_dir, pressure_log, "/mnt/target-x")
            self.assertIn("::error::not-proven-io-failure", output)
            self.assertNotIn("resource-exhaustion", output)
            verdict = pressure_log.read_text(encoding="utf-8")
            self.assertIn("VERDICT: not_proven_io_failure", verdict)
            self.assertIn("does not exonerate the candidate", verdict)
            self.assertNotIn("not candidate defects", verdict)

    def test_lone_linker_sigbus_does_not_exonerate_candidate(self) -> None:
        # Negative control (review #12183): `ld terminated with signal 7`
        # identifies a bus error, not its cause — truncated or replaced mmap
        # inputs, storage I/O faults, corrupt objects, and hardware faults
        # produce it with no ENOSPC anywhere. A lone SIGBUS therefore earns
        # the non-exonerating corroborating verdict, never the definitive one.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            logs_dir = root / "logs"
            logs_dir.mkdir()
            (logs_dir / "unit_routed_full.log").write_text(
                "collect2: fatal error: ld terminated with signal 7 [Bus error]",
                encoding="utf-8",
            )
            pressure_log = root / "pr-fast-disk-pressure.log"
            output = self.run_classify(logs_dir, pressure_log, "/mnt/target-x")
            self.assertIn(
                "::error::not-proven-io-failure [link_sigbus]", output
            )
            self.assertNotIn("resource-exhaustion", output)
            verdict = pressure_log.read_text(encoding="utf-8")
            self.assertIn("VERDICT: not_proven_io_failure", verdict)
            self.assertIn("does not exonerate the candidate", verdict)
            self.assertNotIn("not candidate defects", verdict)

    def test_enospc_alongside_sigbus_still_earns_definitive_verdict(self) -> None:
        # An ENOSPC match proves an occurrence only in its named log.
        # The separate SIGBUS log keeps its weaker annotation and cannot
        # inherit a definitive cause from the strong log.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            logs_dir = root / "logs"
            logs_dir.mkdir()
            (logs_dir / "clippy_full.log").write_text(
                "error: No space left on device (os error 28)",
                encoding="utf-8",
            )
            (logs_dir / "unit_routed_full.log").write_text(
                "collect2: fatal error: ld terminated with signal 7 [Bus error]",
                encoding="utf-8",
            )
            pressure_log = root / "pr-fast-disk-pressure.log"
            output = self.run_classify(logs_dir, pressure_log, "/mnt/target-x")
            self.assertIn("::error::resource-exhaustion [enospc]", output)
            self.assertIn(
                "::error::not-proven-io-failure [link_sigbus]", output
            )
            verdict = pressure_log.read_text(encoding="utf-8")
            self.assertIn("VERDICT: resource-exhaustion detected", verdict)
            self.assertNotIn("not candidate defects", verdict)
            self.assertEqual(
                [line for line in verdict.splitlines() if line.startswith("VERDICT:")],
                [
                    "VERDICT: resource-exhaustion detected in gate logs "
                    "(ENOSPC / os-error-28 class); matched logs: "
                    f"{logs_dir / 'clippy_full.log'}. "
                    "Other gate failure causes remain unattributed."
                ],
            )

    def test_enospc_match_anywhere_still_earns_definitive_verdict(self) -> None:
        # Strong evidence names only the ENOSPC log. The LLVM-only log
        # retains its weaker annotation and is excluded from that scope.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            logs_dir = root / "logs"
            logs_dir.mkdir()
            (logs_dir / "clippy_full.log").write_text(
                "error: No space left on device (os error 28)",
                encoding="utf-8",
            )
            (logs_dir / "unit_routed_full.log").write_text(
                "rustc-LLVM ERROR: IO failure on output stream: broken pipe",
                encoding="utf-8",
            )
            pressure_log = root / "pr-fast-disk-pressure.log"
            output = self.run_classify(logs_dir, pressure_log, "/mnt/target-x")
            self.assertIn("::error::resource-exhaustion [enospc]", output)
            self.assertIn(
                "::error::not-proven-io-failure [llvm_io_failure]", output
            )
            verdict = pressure_log.read_text(encoding="utf-8")
            self.assertIn("VERDICT: resource-exhaustion detected", verdict)
            self.assertNotIn("not candidate defects", verdict)
            self.assertEqual(
                [line for line in verdict.splitlines() if line.startswith("VERDICT:")],
                [
                    "VERDICT: resource-exhaustion detected in gate logs "
                    "(ENOSPC / os-error-28 class); matched logs: "
                    f"{logs_dir / 'clippy_full.log'}. "
                    "Other gate failure causes remain unattributed."
                ],
            )

    def test_mixed_gate_failures_are_not_globally_exonerated(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            logs_dir = root / "logs"
            logs_dir.mkdir()
            (logs_dir / "unit_routed_full.log").write_text(
                "error: No space left on device (os error 28)\n"
                "collect2: fatal error: ld terminated with signal 7 [Bus error]",
                encoding="utf-8",
            )
            (logs_dir / "unit_scoped.log").write_text(
                "assertion failed: expected Ready, got DegradedCancelled\n"
                "test result: FAILED. 2487 passed; 1 failed",
                encoding="utf-8",
            )
            (logs_dir / "source_commit_api_check.log").write_text(
                "source commit API ledger mismatch: ledger705/source706",
                encoding="utf-8",
            )
            pressure_log = root / "pr-fast-disk-pressure.log"
            output = self.run_classify(logs_dir, pressure_log, "/mnt/target-x")
            verdict = pressure_log.read_text(encoding="utf-8")
            self.assertEqual(
                [line for line in verdict.splitlines() if line.startswith("VERDICT:")],
                [
                    "VERDICT: resource-exhaustion detected in gate logs "
                    "(ENOSPC / os-error-28 class); matched logs: "
                    f"{logs_dir / 'unit_routed_full.log'}. "
                    "Other gate failure causes remain unattributed."
                ],
            )
            self.assertIn("::error::resource-exhaustion [enospc]", output)
            self.assertNotIn("unit_scoped.log", output)
            self.assertNotIn("source_commit_api_check.log", output)
            self.assertNotIn("not candidate defects", verdict)

    def test_multiple_strong_logs_have_deterministic_named_scope(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            logs_dir = root / "logs"
            logs_dir.mkdir()
            # Create reverse lexical order; the oracle pins known fixture paths.
            (logs_dir / "z_compile.log").write_text(
                "error: No space left on device", encoding="utf-8"
            )
            (logs_dir / "a_compile.log").write_text(
                "error: failed to write output (os error 28)", encoding="utf-8"
            )
            (logs_dir / "unit_scoped.log").write_text(
                "assertion failed: expected Ready", encoding="utf-8"
            )
            pressure_log = root / "pr-fast-disk-pressure.log"
            output = self.run_classify(logs_dir, pressure_log, "/mnt/target-x")
            verdict = pressure_log.read_text(encoding="utf-8")
            self.assertEqual(
                [line for line in verdict.splitlines() if line.startswith("VERDICT:")],
                [
                    "VERDICT: resource-exhaustion detected in gate logs "
                    "(ENOSPC / os-error-28 class); matched logs: "
                    f"{logs_dir / 'a_compile.log'}, {logs_dir / 'z_compile.log'}. "
                    "Other gate failure causes remain unattributed."
                ],
            )
            self.assertNotIn("unit_scoped.log", output)
            self.assertNotIn("not candidate defects", verdict)

    def test_clean_run_records_absence_of_exhaustion(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            logs_dir = root / "logs"
            logs_dir.mkdir()
            (logs_dir / "fmt.log").write_text("clean", encoding="utf-8")
            pressure_log = root / "pr-fast-disk-pressure.log"
            output = self.run_classify(logs_dir, pressure_log, "target")
            self.assertNotIn("::error::", output)
            self.assertIn("no exhaustion signatures", output)
            self.assertIn("no resource-exhaustion signatures", pressure_log.read_text(encoding="utf-8").lower())

    def test_preparation_log_without_receipt_is_not_full_tier_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            logs_dir = root / "logs"
            logs_dir.mkdir()
            (logs_dir / guard.PREPARATION_GATE_LOG).write_text(
                "all checks passed", encoding="utf-8"
            )
            pressure_log = root / "pr-fast-disk-pressure.log"
            output = self.run_classify(logs_dir, pressure_log, "target")
            self.assertIn("NOT_PROVEN", output)
            self.assertNotIn("no resource-exhaustion signatures found", output.lower())
            verdict = pressure_log.read_text(encoding="utf-8")
            self.assertIn("VERDICT: NOT_PROVEN", verdict)

    def test_pressure_log_alone_is_not_gate_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            logs_dir = root / "logs"
            logs_dir.mkdir()
            (logs_dir / guard.PRESSURE_LOG_NAME).write_text(
                "== PR-fast disk-exhaustion classification ==\n", encoding="utf-8"
            )
            pressure_log = root / "pr-fast-disk-pressure.log"
            output = self.run_classify(logs_dir, pressure_log, "target")
            self.assertIn("NOT_PROVEN", output)
            self.assertNotIn("no resource-exhaustion signatures found", output.lower())
            verdict = pressure_log.read_text(encoding="utf-8")
            self.assertIn("VERDICT: NOT_PROVEN", verdict)

    def test_missing_pressure_log_parent_is_created(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            nested = root / "a" / "b" / "pr-fast-disk-pressure.log"
            self.run_classify(root / "absent-logs", nested, "target")
            self.assertTrue(nested.exists())


class PreflightPressureLogCompositionTests(unittest.TestCase):
    def test_breach_appends_rejection_record_with_snapshot(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            pressure_log = root / "target" / "receipts" / "logs" / "x.log"

            # Reuse the module's append helper directly to pin the contract
            # classify/preflight share: best-effort, never raises.
            guard._append_pressure_record(
                pressure_log,
                header="== header ==",
                body_lines=["line-one"],
            )
            self.assertIn("== header ==", pressure_log.read_text(encoding="utf-8"))
            self.assertIn("line-one", pressure_log.read_text(encoding="utf-8"))

    def test_unwritable_pressure_log_warns_instead_of_raising(self) -> None:
        buffer = io.StringIO()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            # A regular file occupying the parent slot makes the helper's
            # mkdir(parents=True) raise NotADirectoryError on every platform,
            # without mocks, host-specific drives, or workspace litter.
            blocker = root / "occupied"
            blocker.write_text("not a directory", encoding="utf-8")
            with redirect_stdout(buffer):
                guard._append_pressure_record(
                    blocker / "logs" / "pr-fast-disk-pressure.log",
                    header="h",
                    body_lines=[],
                )
        self.assertIn("::warning::could not update", buffer.getvalue())


class RestoredGateLogCleanupTests(unittest.TestCase):
    def run_preflight(self, workspace: Path) -> str:
        buffer = io.StringIO()
        with redirect_stdout(buffer):
            status = guard.run_preflight(
                argparse.Namespace(
                    workspace_path=str(workspace),
                    target_dir="",
                    min_free_bytes=1,
                    footprint_bytes=0,
                    reserve_bytes=0,
                )
            )
        self.assertEqual(status, 0)
        return buffer.getvalue()

    def test_stale_enospc_log_plus_clean_current_logs_classify_clean(self) -> None:
        # Negative control for the misattribution class (review #12183): the
        # shared cargo cache restores ``target`` wholesale, so this run starts
        # with an unrelated attempt's ENOSPC log. Preflight must sweep it
        # before anything writes today's records; afterwards the gate writes
        # only clean logs and classification finds no exhaustion signature.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            logs_dir = root / "target" / "receipts" / "logs"
            logs_dir.mkdir(parents=True)
            (logs_dir / "unit_routed_full.log").write_text(
                "error: No space left on device (os error 28)",
                encoding="utf-8",
            )

            self.run_preflight(root)

            # The current run's own (clean) gate logs are written afterwards.
            (logs_dir / "fmt.log").write_text(
                "all checks passed", encoding="utf-8"
            )

            self.assertNotIn(
                "No space left",
                "\n".join(
                    path.read_text(encoding="utf-8") for path in logs_dir.glob("*.log")
                ),
            )
            self.assertEqual(guard.scan_logs(logs_dir), [])
            provenance = (logs_dir / guard.PRESSURE_LOG_NAME).read_text(
                encoding="utf-8"
            )
            self.assertIn(
                "cleared 1 restored log(s): unit_routed_full.log", provenance
            )

    def test_cleanup_tolerates_occupied_logs_dir(self) -> None:
        # A regular file occupying the receipt-log slot must not break the
        # advisory preflight lane; the sweep declines and preflight proceeds.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            blocker = root / "target" / "receipts" / "logs"
            blocker.parent.mkdir(parents=True)
            blocker.write_text("not a directory", encoding="utf-8")

            self.run_preflight(root)


if __name__ == "__main__":
    unittest.main()
