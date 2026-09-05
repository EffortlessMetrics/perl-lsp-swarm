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
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

_SCRIPTS_DIR = Path(__file__).parent
_REPO_ROOT = _SCRIPTS_DIR.parent.parent.resolve()
_FORMAT_RESULTS = _SCRIPTS_DIR / "format-results.py"
_COMPARE = _SCRIPTS_DIR / "compare.py"
_SIDECAR = _SCRIPTS_DIR / "validate-native-pipeline-sidecar.py"
_WORKFLOW = _SCRIPTS_DIR.parent.parent / ".github" / "workflows" / "ci-nightly.yml"
_COUNTERS_RS = (
    _REPO_ROOT / "crates" / "perl-lsp-perltidy" / "src" / "native" / "counters.rs"
)


def _rust_counter_clock_tag() -> str:
    source = _COUNTERS_RS.read_text(encoding="utf-8")
    match = re.search(
        r'^pub const COUNTER_CLOCK_TAG: &str = "(?P<tag>[^"]+)";',
        source,
        re.MULTILINE,
    )
    if match is None:
        raise AssertionError("Rust counter clock contract is missing")
    return match.group("tag")


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
            results_file.write_text(
                json.dumps(
                    {
                        "version": "0.9.0",
                        "timestamp": "2026-07-12T00:00:00Z",
                        "git_sha": "abc123",
                        "results": {},
                    }
                )
            )

            proc = _run(_FORMAT_RESULTS, [str(results_file), "--receipt"])
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("Total benchmarks:  0", proc.stdout)
            self.assertIn("STATUS: INVALID", proc.stdout)
            self.assertNotIn("STATUS: COMPLETE", proc.stdout)

    def test_nonzero_benchmarks_is_complete_and_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            results_file = Path(tmp) / "latest.json"
            results_file.write_text(
                json.dumps(
                    {
                        "version": "0.9.0",
                        "timestamp": "2026-07-12T00:00:00Z",
                        "git_sha": "abc123",
                        "results": {
                            "index": {
                                "incremental update single file": {"mean_ns": 209_000},
                            },
                        },
                    }
                )
            )

            proc = _run(_FORMAT_RESULTS, [str(results_file), "--receipt"])
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            self.assertIn("Total benchmarks:  1", proc.stdout)
            self.assertIn("STATUS: COMPLETE", proc.stdout)


class CompareMissingBenchmarkGuardTests(unittest.TestCase):
    def _write(self, path: Path, results: dict) -> None:
        path.write_text(
            json.dumps(
                {
                    "version": "0.10.0",
                    "timestamp": "2026-07-12T00:00:00Z",
                    "git_sha": "deadbeef",
                    "results": results,
                }
            )
        )

    def test_missing_expected_benchmark_fails_under_fail_on_regression(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "baseline.json"
            current = Path(tmp) / "current.json"
            self._write(
                baseline,
                {
                    "index": {"incremental update single file": {"mean_ns": 209_000}},
                },
            )
            # Current run produced nothing for the "index" category at all --
            # the baseline's benchmark is MISSING, not regressed.
            self._write(current, {})

            proc = _run(_COMPARE, [str(baseline), str(current), "--fail-on-regression"])
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("Missing:", proc.stdout)

    def test_missing_expected_benchmark_is_advisory_without_fail_on_regression(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "baseline.json"
            current = Path(tmp) / "current.json"
            self._write(
                baseline,
                {
                    "index": {"incremental update single file": {"mean_ns": 209_000}},
                },
            )
            self._write(current, {})

            proc = _run(_COMPARE, [str(baseline), str(current)])
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)

    def test_matching_benchmark_with_no_regression_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "baseline.json"
            current = Path(tmp) / "current.json"
            self._write(
                baseline,
                {
                    "index": {"incremental update single file": {"mean_ns": 209_000}},
                },
            )
            self._write(
                current,
                {
                    "index": {"incremental update single file": {"mean_ns": 210_000}},
                },
            )

            proc = _run(_COMPARE, [str(baseline), str(current), "--fail-on-regression"])
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            self.assertIn("STATUS: PASS", proc.stdout)

    def test_real_regression_fails_under_fail_on_regression(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "baseline.json"
            current = Path(tmp) / "current.json"
            self._write(
                baseline,
                {
                    "index": {"incremental update single file": {"mean_ns": 209_000}},
                },
            )
            # +50%, well over the 20% regression threshold.
            self._write(
                current,
                {
                    "index": {"incremental update single file": {"mean_ns": 314_000}},
                },
            )

            proc = _run(_COMPARE, [str(baseline), str(current), "--fail-on-regression"])
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("REGRESSION DETECTED", proc.stdout)


class NightlyWorkflowGuardTests(unittest.TestCase):
    def test_nightly_bench_exports_rustc_derived_toolchain_tag(self) -> None:
        workflow = _WORKFLOW.read_text(encoding="utf-8")
        match = re.search(
            r"(?ms)^    - name: Run benchmarks \(explicit Criterion targets\)\n"
            r"(?P<step>.*?)(?=^    - name:|\Z)",
            workflow,
        )
        self.assertIsNotNone(match, "nightly benchmark step is missing")
        step = match.group("step")
        self.assertIn("rustc -vV", step)
        self.assertRegex(step, r'RUSTC_RELEASE=.*sed .*release:')
        self.assertRegex(step, r'RUSTC_COMMIT_HASH=.*sed .*commit-hash:')
        self.assertRegex(
            step,
            r'export NATIVE_PIPELINE_TOOLCHAIN_TAG="rustc-\$\{RUSTC_RELEASE\}-'
            r'\$\{RUSTC_COMMIT_HASH\}-\$\{RUSTC_ARCH\}-\$\{RUSTC_OS\}"',
        )


class NativePipelineSidecarGuardTests(unittest.TestCase):
    EXPECTED = [
        "native_pipeline_document/delimited_n8_lf_tabs",
        "native_pipeline_document/delimited_n32_lf_tabs",
    ]
    RUN_ID = "123-abc"

    def _row(
        self,
        bench_id: str,
        run_id: str | None = None,
        schema: str = "native-pipeline-counters-v1",
    ) -> dict:
        return {
            "schema": schema,
            "run_id": run_id or self.RUN_ID,
            "bench_id": bench_id,
            "toolchain": "rustc-test-toolchain",
            "counters": {
                "schema": "native-pipeline-counters-v1",
                "pipeline_invocations": 1,
                "parse_gate_invocations": 2,
                "source_parse_gate_invocations": 1,
                "formatted_output_parse_gate_invocations": 1,
                "gate_nodes_observed": 3,
                "lines_processed": 4,
                "layout_groups_fitted": 1,
                "edits_derived": 1,
                "replacement_bytes": 8,
                "peak_depth": 1,
                "elapsed": {"secs": 0, "nanos": 42},
                "clock_tag": _rust_counter_clock_tag(),
            },
        }

    def test_identity_only_sidecar_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            rows = [
                {
                    "schema": "native-pipeline-counters-v1",
                    "run_id": self.RUN_ID,
                    "bench_id": bench_id,
                }
                for bench_id in self.EXPECTED
            ]
            path = self._write(Path(tmp), rows)
            proc = self._run(path)
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("counter snapshot", proc.stderr)

    def test_counter_snapshot_without_a_required_field_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            rows = [self._row(bench_id) for bench_id in self.EXPECTED]
            rows[0]["counters"].pop("lines_processed")
            path = self._write(Path(tmp), rows)
            proc = self._run(path)
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("lines_processed", proc.stderr)

    def test_counter_snapshot_without_clock_tag_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            rows = [self._row(bench_id) for bench_id in self.EXPECTED]
            rows[0]["counters"].pop("clock_tag")
            path = self._write(Path(tmp), rows)
            proc = self._run(path)
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("clock_tag", proc.stderr)

    def test_row_without_toolchain_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            rows = [self._row(bench_id) for bench_id in self.EXPECTED]
            rows[0].pop("toolchain")
            path = self._write(Path(tmp), rows)
            proc = self._run(path)
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("toolchain", proc.stderr)

    def test_row_with_blank_toolchain_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            rows = [self._row(bench_id) for bench_id in self.EXPECTED]
            rows[0]["toolchain"] = ""
            path = self._write(Path(tmp), rows)
            proc = self._run(path)
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("toolchain", proc.stderr)

    def test_counter_snapshot_with_wrong_clock_tag_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            rows = [self._row(bench_id) for bench_id in self.EXPECTED]
            rows[0]["counters"]["clock_tag"] = "wall-clock"
            path = self._write(Path(tmp), rows)
            proc = self._run(path)
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("clock_tag", proc.stderr)

    def test_zero_pipeline_invocations_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            rows = [self._row(bench_id) for bench_id in self.EXPECTED]
            rows[0]["counters"]["pipeline_invocations"] = 0
            path = self._write(Path(tmp), rows)
            proc = self._run(path)
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("pipeline_invocations must be positive", proc.stderr)

    def test_unnormalised_elapsed_nanos_are_rejected(self) -> None:
        """serde normalises `Duration`, so nanos is always a sub-second remainder.

        A value at or above one billion cannot have come from a real
        `std::time::Duration`, which makes it fabricated or corrupted evidence
        rather than a slow run. Without this the guard accepted an arbitrary
        integer in the nanos field and still reported the receipt as valid.
        """
        with tempfile.TemporaryDirectory() as tmp:
            rows = [self._row(bench_id) for bench_id in self.EXPECTED]
            rows[0]["counters"]["elapsed"] = {"secs": 0, "nanos": 1_000_000_000}
            path = self._write(Path(tmp), rows)
            proc = self._run(path)
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("sub-second remainder", proc.stderr)

    def test_maximum_normalised_elapsed_nanos_are_accepted(self) -> None:
        """The boundary's admitting side: one nanosecond below the limit is a
        legitimate `Duration` and must still pass, so the rule above rejects
        malformed evidence rather than simply rejecting large elapsed values."""
        with tempfile.TemporaryDirectory() as tmp:
            rows = [self._row(bench_id) for bench_id in self.EXPECTED]
            rows[0]["counters"]["elapsed"] = {"secs": 7, "nanos": 999_999_999}
            path = self._write(Path(tmp), rows)
            proc = self._run(path)
            self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_zero_non_invocation_counters_are_accepted_as_shape_only(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            rows = [self._row(bench_id) for bench_id in self.EXPECTED]
            rows[0]["counters"].update(
                {
                    "parse_gate_invocations": 0,
                    "source_parse_gate_invocations": 0,
                    "formatted_output_parse_gate_invocations": 0,
                    "gate_nodes_observed": 0,
                    "lines_processed": 0,
                    "layout_groups_fitted": 0,
                    "edits_derived": 0,
                    "replacement_bytes": 0,
                    "peak_depth": 0,
                }
            )
            path = self._write(Path(tmp), rows)
            proc = self._run(path)
            self.assertEqual(proc.returncode, 0, proc.stderr)

    def _run(
        self, sidecar: Path, expected_ids: list[str] | None = None
    ) -> subprocess.CompletedProcess:
        args = [
            sys.executable,
            str(_SIDECAR),
            "--sidecar",
            str(sidecar),
            "--expected-run-id",
            self.RUN_ID,
        ]
        for bench_id in expected_ids or self.EXPECTED:
            args.extend(["--expect-id", bench_id])
        return subprocess.run(args, capture_output=True, text=True, check=False)

    def _write(
        self,
        directory: Path,
        rows: list[dict],
        schema: str = "native-pipeline-measurements-v1",
    ) -> Path:
        path = directory / "sidecar.json"
        path.write_text(
            json.dumps({"schema": schema, "run_id": self.RUN_ID, "subjects": rows})
        )
        return path

    def test_complete_sidecar_is_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(
                Path(tmp), [self._row(bench_id) for bench_id in self.EXPECTED]
            )
            proc = self._run(path)
            self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_rust_producer_clock_contract_is_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(
                Path(tmp), [self._row(bench_id) for bench_id in self.EXPECTED]
            )
            proc = self._run(path)
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)

    def test_missing_sidecar_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            proc = self._run(Path(tmp) / "missing.json")
            self.assertNotEqual(proc.returncode, 0)

    def test_schema_and_run_id_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(
                Path(tmp),
                [self._row(bench_id, run_id="stale") for bench_id in self.EXPECTED],
                schema="wrong",
            )
            proc = self._run(path)
            self.assertNotEqual(proc.returncode, 0)

    def test_duplicate_or_missing_enrollment_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(
                Path(tmp), [self._row(self.EXPECTED[0]), self._row(self.EXPECTED[0])]
            )
            proc = self._run(path)
            self.assertNotEqual(proc.returncode, 0)

    def test_workflow_style_array_invocation_accepts_valid_sidecar(self) -> None:
        workflow = _WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("sidecar_args=()", workflow)
        self.assertIn('sidecar_args+=(--expect-id "$expected_id")', workflow)
        self.assertIn('"${sidecar_args[@]}"', workflow)

        with tempfile.TemporaryDirectory(dir=_REPO_ROOT) as tmp:
            path = self._write(
                Path(tmp), [self._row(bench_id) for bench_id in self.EXPECTED]
            )
            expected_ids = [
                "native_pipeline_document/delimited_n8_lf_tabs",
                "native_pipeline_document/delimited_n32_lf_tabs",
            ]
            sidecar_args = [
                arg
                for expected_id in expected_ids
                for arg in ("--expect-id", expected_id)
            ]
            proc = subprocess.run(
                [
                    sys.executable,
                    str(_SIDECAR.resolve()),
                    "--sidecar",
                    str(path.resolve()),
                    "--expected-run-id",
                    self.RUN_ID,
                    *sidecar_args,
                ],
                capture_output=True,
                text=True,
                check=False,
                cwd=_REPO_ROOT,
            )
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)


if __name__ == "__main__":
    unittest.main()
