#!/usr/bin/env python3
"""Proof that the nightly Criterion targets are explicit and runnable.

Run directly (stdlib only)::

    python3 benchmarks/scripts/test_benchmark_targets.py -v

The positive checks join Cargo metadata to the affected manifest entries and
their Criterion entry points.  The negative controls exercise the authority
boundary itself and Cargo's libtest behavior: restoring ``harness = true``
must not accept the Criterion-only ``--noplot`` argument.
"""

import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - the CI Python is 3.11+
    import tomli as tomllib  # type: ignore[no-redef]


ROOT = Path(__file__).resolve().parents[2]
EXPECTED_TARGETS = {
    "perl-parser": {
        "positions_bench": "crates/perl-parser/benches/positions_bench.rs",
        "substitution_performance": "crates/perl-parser/benches/substitution_performance.rs",
    },
    "perl-incremental-parsing": {
        "incremental_parsing_benchmarks": (
            "crates/perl-incremental-parsing/benches/"
            "incremental_parsing_benchmarks.rs"
        ),
    },
}


def _manifest(crate: str) -> dict:
    path = ROOT / "crates" / crate / "Cargo.toml"
    with path.open("rb") as handle:
        return tomllib.load(handle)


def _bench_entry(manifest: dict, name: str) -> dict:
    entries = [entry for entry in manifest.get("bench", []) if entry.get("name") == name]
    if len(entries) != 1:
        raise AssertionError(f"expected exactly one [[bench]] entry for {name!r}")
    return entries[0]


def _require_criterion_target(entry: dict, source: str) -> None:
    if entry.get("harness") is not False:
        raise AssertionError("Criterion targets must explicitly set harness = false")
    if "criterion_main!" not in source:
        raise AssertionError("declared target does not expose a Criterion main")


class BenchmarkTargetAuthorityTests(unittest.TestCase):
    def test_affected_targets_are_declared_exposed_and_criterion_backed(self) -> None:
        metadata = json.loads(
            subprocess.check_output(
                [
                    "cargo",
                    "metadata",
                    "--manifest-path",
                    str(ROOT / "Cargo.toml"),
                    "--no-deps",
                    "--format-version",
                    "1",
                    "--locked",
                ],
                cwd=ROOT,
                text=True,
            )
        )
        packages = {package["name"]: package for package in metadata["packages"]}

        for crate, targets in EXPECTED_TARGETS.items():
            manifest = _manifest(crate)
            package = packages[crate]
            metadata_targets = {
                target["name"]: target
                for target in package["targets"]
                if "bench" in target["kind"]
            }
            for name, source_path in targets.items():
                entry = _bench_entry(manifest, name)
                source = (ROOT / source_path).read_text(encoding="utf-8")
                _require_criterion_target(entry, source)
                self.assertIn(name, metadata_targets)

    def test_authority_rejects_harness_true_negative_control(self) -> None:
        manifest = _manifest("perl-parser")
        entry = copy.deepcopy(_bench_entry(manifest, "positions_bench"))
        entry["harness"] = True
        source = (ROOT / EXPECTED_TARGETS["perl-parser"]["positions_bench"]).read_text(
            encoding="utf-8"
        )
        with self.assertRaisesRegex(AssertionError, "harness = false"):
            _require_criterion_target(entry, source)

    def test_authority_rejects_non_criterion_negative_control(self) -> None:
        manifest = _manifest("perl-parser")
        entry = _bench_entry(manifest, "positions_bench")
        with self.assertRaisesRegex(AssertionError, "Criterion main"):
            _require_criterion_target(entry, "fn main() {}")


class CargoHarnessNegativeControlTests(unittest.TestCase):
    def test_libtest_harness_rejects_noplot(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "benches").mkdir()
            (root / "Cargo.toml").write_text(
                '[package]\nname = "harness-control"\nversion = "0.0.0"\nedition = "2021"\n',
                encoding="utf-8",
            )
            (root / "benches" / "criterion_like.rs").write_text(
                "fn main() {}\n",
                encoding="utf-8",
            )
            process = subprocess.run(
                [
                    "cargo",
                    "bench",
                    "--manifest-path",
                    str(root / "Cargo.toml"),
                    "--bench",
                    "criterion_like",
                    "--",
                    "--noplot",
                ],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertNotEqual(process.returncode, 0)
            self.assertIn("Unrecognized option: 'noplot'", process.stdout + process.stderr)


if __name__ == "__main__":
    unittest.main()
