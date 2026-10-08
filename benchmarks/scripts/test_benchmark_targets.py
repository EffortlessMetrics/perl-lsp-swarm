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
import importlib.util
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
        self.assertEqual(len(nightly), 16)
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


def _load_sibling_module(filename: str):
    path = Path(__file__).resolve().parent / filename
    name = "bench_" + path.stem.replace("-", "_")
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise AssertionError(f"cannot load sibling script {filename}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _workflow_expect_ids() -> list[str]:
    text = WORKFLOW.read_text(encoding="utf-8")
    ids = re.findall(r'--expect-id "([^"]+)"', text)
    # Skip loop-variable placeholders; only static declared ids are auditable.
    return [i for i in ids if not i.startswith("$")]


def _latest_baseline() -> dict:
    # Use the comparator's own selector: nightly compares against the
    # semver-latest file (compare.find_latest_baseline), not the mtime-latest,
    # so the guard must read the same file the comparator will.
    compare = _load_sibling_module("compare.py")
    latest = compare.find_latest_baseline(ROOT)
    if latest is None:
        raise AssertionError("no nightly baseline file found")
    return json.loads(latest.read_text(encoding="utf-8"))


# Exact ripr rows the comparator joins on, as extract-criterion.py spells them
# (verified by running the extractor: unique names unqualified, spanning names
# qualified). Any row deletion, addition, or rename fails loudly so baseline
# edits stay conscious.
EXPECTED_RIPR_BASELINE_KEYS = frozenset(
    {
        "small",
        "medium",
        "packet_build/large",
        "packet_fingerprint/large",
    }
)


def _validate_ripr_baseline_coverage(baseline: dict | None = None) -> None:
    """Every ripr expect-id must categorize into a baseline category (#17355).

    Uses the production classifier and alias map, so the guard tracks the real
    nightly join instead of duplicating its rules.
    """
    extract = _load_sibling_module("extract-criterion.py")
    compare = _load_sibling_module("compare.py")
    if baseline is None:
        baseline = _latest_baseline()
    baseline_cats = {
        compare.normalize_category_name(c) for c in baseline["benchmarks"]
    }
    ripr_ids = [i for i in _workflow_expect_ids() if "packet" in i or "ripr" in i]
    if not ripr_ids:
        raise AssertionError("nightly workflow must declare ripr expect-ids")
    for expect_id in ripr_ids:
        if "/" in expect_id:
            group, name = expect_id.split("/", 1)
        else:
            group, name = "other", expect_id
        category = compare.normalize_category_name(
            extract.categorize_benchmark(group, name)
        )
        if category not in baseline_cats:
            raise AssertionError(
                f"ripr expect-id {expect_id!r} categorizes to {category!r}, "
                "which has no baseline category; nightly compare/alert would "
                "silently skip it (#17355)"
            )
    ripr_rows = set(baseline["benchmarks"].get("ripr", {}).keys())
    if ripr_rows != EXPECTED_RIPR_BASELINE_KEYS:
        raise AssertionError(
            f"ripr baseline rows {sorted(ripr_rows)} do not match the "
            f"extractor-emitted key set {sorted(EXPECTED_RIPR_BASELINE_KEYS)}; "
            "a deleted, added, or renamed row would silently skip "
            "comparison (#17355)"
        )


class BaselineCoverageTests(unittest.TestCase):
    def test_ripr_expect_ids_have_baseline_categories(self) -> None:
        _validate_ripr_baseline_coverage()

    def test_missing_ripr_category_is_rejected(self) -> None:
        baseline = copy.deepcopy(_latest_baseline())
        baseline["benchmarks"].pop("ripr", None)
        with self.assertRaisesRegex(AssertionError, "no baseline category"):
            _validate_ripr_baseline_coverage(baseline)

    def test_missing_ripr_row_is_rejected(self) -> None:
        baseline = copy.deepcopy(_latest_baseline())
        baseline["benchmarks"]["ripr"].pop("small", None)
        with self.assertRaisesRegex(AssertionError, "do not match"):
            _validate_ripr_baseline_coverage(baseline)


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
