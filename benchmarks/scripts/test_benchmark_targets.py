#!/usr/bin/env python3
"""Proof that every nightly benchmark target is explicit and runnable.

Run directly with Python 3.11 or newer::

    python3 benchmarks/scripts/test_benchmark_targets.py -v

The positive check reconciles the workflow target list with Cargo metadata,
including each target's source path and required features, then checks the
manifest and Criterion entry point. Negative controls prove that wrong path or
feature authority and the legacy libtest harness are rejected.
"""

import copy
import json
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

import tomllib


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/ci-nightly.yml"


def _workflow_targets() -> list[tuple[str, str, str]]:
    text = WORKFLOW.read_text(encoding="utf-8")
    match = re.search(
        r"declare -a BENCH_TARGETS=\(\n(?P<body>.*?)\n\s*\)",
        text,
        re.DOTALL,
    )
    if match is None:
        raise AssertionError("nightly BENCH_TARGETS declaration is missing")

    entries = re.findall(r'^\s*"([^"]+)"\s*$', match.group("body"), re.MULTILINE)
    targets = []
    for entry in entries:
        parts = entry.split(":", 2)
        if len(parts) != 3 or not all(parts[:2]):
            raise AssertionError(f"invalid nightly benchmark target entry: {entry!r}")
        targets.append((parts[0], parts[1], parts[2]))
    return targets


def _metadata() -> dict:
    return json.loads(
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


def _manifest(crate: str) -> tuple[Path, dict]:
    path = ROOT / "crates" / crate / "Cargo.toml"
    with path.open("rb") as handle:
        return path, tomllib.load(handle)


def _metadata_benches(metadata: dict) -> dict[tuple[str, str], dict]:
    result = {}
    for package in metadata["packages"]:
        for target in package["targets"]:
            if "bench" in target["kind"]:
                result[(package["name"], target["name"])] = target
    return result


def _manifest_bench(manifest: dict, name: str) -> dict:
    entries = [entry for entry in manifest.get("bench", []) if entry.get("name") == name]
    if len(entries) != 1:
        raise AssertionError(f"expected one [[bench]] entry for {name!r}")
    return entries[0]


def _require_criterion_target(entry: dict, source: str) -> None:
    if entry.get("harness") is not False:
        raise AssertionError("Criterion targets must explicitly set harness = false")
    has_macro_main = "criterion_main!" in source
    has_manual_main = all(
        marker in source
        for marker in ("Criterion::default()", "configure_from_args", "final_summary")
    )
    if not (has_macro_main or has_manual_main):
        raise AssertionError("declared target does not expose a Criterion entry point")


def _validate_targets(
    nightly: list[tuple[str, str, str]], metadata: dict
) -> None:
    nightly_keys = [(crate, name) for crate, name, _ in nightly]
    if len(nightly_keys) != len(set(nightly_keys)):
        raise AssertionError("nightly benchmark target list contains duplicates")

    metadata_benches = _metadata_benches(metadata)
    if set(nightly_keys) != set(metadata_benches):
        missing = sorted(set(metadata_benches) - set(nightly_keys))
        extra = sorted(set(nightly_keys) - set(metadata_benches))
        raise AssertionError(f"nightly/metadata mismatch: missing={missing}, extra={extra}")

    for crate, name, feature_text in nightly:
        target = metadata_benches[(crate, name)]
        required_features = tuple(target.get("required-features", []))
        requested_features = tuple(filter(None, feature_text.split(",")))
        if requested_features != required_features:
            raise AssertionError(
                f"feature mismatch for {crate}:{name}: "
                f"nightly={requested_features}, metadata={required_features}"
            )

        manifest_path, manifest = _manifest(crate)
        entry = _manifest_bench(manifest, name)
        declared_path = manifest_path.parent / entry.get("path", f"benches/{name}.rs")
        metadata_path = Path(target["src_path"])
        if metadata_path.resolve() != declared_path.resolve():
            raise AssertionError(
                f"source mismatch for {crate}:{name}: "
                f"metadata={metadata_path}, manifest={declared_path}"
            )
        source = metadata_path.read_text(encoding="utf-8")
        _require_criterion_target(entry, source)


class BenchmarkTargetAuthorityTests(unittest.TestCase):
    def test_all_nightly_targets_match_metadata_paths_features_and_sources(self) -> None:
        nightly = _workflow_targets()
        self.assertEqual(len(nightly), 14)
        _validate_targets(nightly, _metadata())

    def test_wrong_src_path_is_rejected(self) -> None:
        nightly = _workflow_targets()
        metadata = _metadata()
        target = _metadata_benches(metadata)[("perl-parser", "positions_bench")]
        target["src_path"] = str(ROOT / "crates/perl-parser/benches/parser_benchmark.rs")
        with self.assertRaisesRegex(AssertionError, "source mismatch"):
            _validate_targets(nightly, metadata)

    def test_wrong_required_feature_is_rejected(self) -> None:
        nightly = _workflow_targets()
        metadata = _metadata()
        target = _metadata_benches(metadata)[("perl-parser", "positions_bench")]
        target["required-features"] = ["synthetic_feature"]
        with self.assertRaisesRegex(AssertionError, "feature mismatch"):
            _validate_targets(nightly, metadata)

    def test_harness_true_is_rejected(self) -> None:
        manifest = _manifest("perl-parser")[1]
        entry = copy.deepcopy(_manifest_bench(manifest, "positions_bench"))
        entry["harness"] = True
        source = (ROOT / "crates/perl-parser/benches/positions_bench.rs").read_text(
            encoding="utf-8"
        )
        with self.assertRaisesRegex(AssertionError, "harness = false"):
            _require_criterion_target(entry, source)


class CargoHarnessNegativeControlTests(unittest.TestCase):
    def test_libtest_harness_rejects_noplot(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "benches").mkdir()
            (root / "Cargo.toml").write_text(
                "[package]\nname = \"harness-control\"\nversion = \"0.0.0\"\n"
                "edition = \"2021\"\n\n[[bench]]\nname = \"criterion_like\"\n"
                "harness = true\n",
                encoding="utf-8",
            )
            (root / "benches" / "criterion_like.rs").write_text(
                "fn main() {}\n", encoding="utf-8"
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
            output = process.stdout + process.stderr
            self.assertNotEqual(process.returncode, 0)
            self.assertIn("noplot", output.casefold())


if __name__ == "__main__":
    unittest.main()
