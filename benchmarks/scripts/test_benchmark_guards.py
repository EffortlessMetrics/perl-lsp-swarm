#!/usr/bin/env python3
"""CLI-level fixture tests for the benchmark integrity guards in
format-results.py and compare.py (#3979).

Run directly (no third-party deps -- stdlib unittest only):

    python3 benchmarks/scripts/test_benchmark_guards.py

Both scripts previously reported success unconditionally: format-results.py
printed "STATUS: COMPLETE" even at a total of zero benchmarks, and
compare.py returned 0 whenever there were no *regressions*, even if every
baseline-expected benchmark was silently MISSING from the current run.
Under a fail-closed benchmark harness, both of those are integrity bugs in
their own right -- they would keep reporting a healthy comparison even
after the extraction step above starts failing closed, if this script were
ever invoked by hand against an old/partial results file.
"""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

_SCRIPTS_DIR = Path(__file__).parent
_FORMAT_RESULTS = _SCRIPTS_DIR / "format-results.py"
_COMPARE = _SCRIPTS_DIR / "compare.py"
_SIDECAR = _SCRIPTS_DIR / "validate-native-pipeline-sidecar.py"


def _run(script: Path, args: "list[str]") -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(script), *args],
        capture_output=True,
        text=True,
        check=False,
    )


class FormatResultsReceiptGuardTests(unittest.TestCase):
    def test_zero_benchmarks_is_invalid_and_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            results_file = Path(tmp) / "latest.json"
            results_file.write_text(json.dumps({
                "version": "0.9.0",
                "timestamp": "2026-07-12T00:00:00Z",
                "git_sha": "abc123",
                "results": {},
            }))

            proc = _run(_FORMAT_RESULTS, [str(results_file), "--receipt"])
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("Total benchmarks:  0", proc.stdout)
            self.assertIn("STATUS: INVALID", proc.stdout)
            self.assertNotIn("STATUS: COMPLETE", proc.stdout)

    def test_nonzero_benchmarks_is_complete_and_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            results_file = Path(tmp) / "latest.json"
            results_file.write_text(json.dumps({
                "version": "0.9.0",
                "timestamp": "2026-07-12T00:00:00Z",
                "git_sha": "abc123",
                "results": {
                    "index": {
                        "incremental update single file": {"mean_ns": 209_000},
                    },
                },
            }))

            proc = _run(_FORMAT_RESULTS, [str(results_file), "--receipt"])
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            self.assertIn("Total benchmarks:  1", proc.stdout)
            self.assertIn("STATUS: COMPLETE", proc.stdout)


class CompareMissingBenchmarkGuardTests(unittest.TestCase):
    def _write(self, path: Path, results: dict) -> None:
        path.write_text(json.dumps({
            "version": "0.10.0",
            "timestamp": "2026-07-12T00:00:00Z",
            "git_sha": "deadbeef",
            "results": results,
        }))

    def test_missing_expected_benchmark_fails_under_fail_on_regression(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "baseline.json"
            current = Path(tmp) / "current.json"
            self._write(baseline, {
                "index": {"incremental update single file": {"mean_ns": 209_000}},
            })
            # Current run produced nothing for the "index" category at all --
            # the baseline's benchmark is MISSING, not regressed.
            self._write(current, {})

            proc = _run(_COMPARE, [str(baseline), str(current), "--fail-on-regression"])
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("Missing:", proc.stdout)

    def test_missing_expected_benchmark_is_advisory_without_fail_on_regression(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "baseline.json"
            current = Path(tmp) / "current.json"
            self._write(baseline, {
                "index": {"incremental update single file": {"mean_ns": 209_000}},
            })
            self._write(current, {})

            proc = _run(_COMPARE, [str(baseline), str(current)])
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)

    def test_matching_benchmark_with_no_regression_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "baseline.json"
            current = Path(tmp) / "current.json"
            self._write(baseline, {
                "index": {"incremental update single file": {"mean_ns": 209_000}},
            })
            self._write(current, {
                "index": {"incremental update single file": {"mean_ns": 210_000}},
            })

            proc = _run(_COMPARE, [str(baseline), str(current), "--fail-on-regression"])
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            self.assertIn("STATUS: PASS", proc.stdout)

    def test_real_regression_fails_under_fail_on_regression(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "baseline.json"
            current = Path(tmp) / "current.json"
            self._write(baseline, {
                "index": {"incremental update single file": {"mean_ns": 209_000}},
            })
            # +50%, well over the 20% regression threshold.
            self._write(current, {
                "index": {"incremental update single file": {"mean_ns": 314_000}},
            })

            proc = _run(_COMPARE, [str(baseline), str(current), "--fail-on-regression"])
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("REGRESSION DETECTED", proc.stdout)


class NativePipelineSidecarGuardTests(unittest.TestCase):
    EXPECTED = ["native_pipeline_document/delimited_n8_lf_tabs",
                "native_pipeline_document/delimited_n32_lf_tabs"]
    RUN_ID = "123-abc"

    def _row(self, bench_id: str, run_id: str | None = None, schema: str = "native-pipeline-counters-v1") -> dict:
        return {"schema": schema, "run_id": run_id or self.RUN_ID, "bench_id": bench_id}

    def _run(self, sidecar: Path, expected_ids: list[str] | None = None) -> subprocess.CompletedProcess:
        args = [sys.executable, str(_SIDECAR), "--sidecar", str(sidecar),
                "--expected-run-id", self.RUN_ID]
        for bench_id in expected_ids or self.EXPECTED:
            args.extend(["--expect-id", bench_id])
        return subprocess.run(args, capture_output=True, text=True, check=False)

    def _write(self, directory: Path, rows: list[dict], schema: str = "native-pipeline-measurements-v1") -> Path:
        path = directory / "sidecar.json"
        path.write_text(json.dumps({"schema": schema, "run_id": self.RUN_ID, "subjects": rows}))
        return path

    def test_complete_sidecar_is_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(Path(tmp), [self._row(bench_id) for bench_id in self.EXPECTED])
            proc = self._run(path)
            self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_missing_sidecar_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(Path(tmp) / "missing.json")
            self.assertNotEqual(proc.returncode, 0)

    def test_schema_and_run_id_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(Path(tmp), [self._row(bench_id, run_id="stale") for bench_id in self.EXPECTED], schema="wrong")
            proc = self._run(path)
            self.assertNotEqual(proc.returncode, 0)

    def test_duplicate_or_missing_enrollment_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(Path(tmp), [self._row(self.EXPECTED[0]), self._row(self.EXPECTED[0])])
            proc = self._run(path)
            self.assertNotEqual(proc.returncode, 0)


if __name__ == "__main__":
    unittest.main()
