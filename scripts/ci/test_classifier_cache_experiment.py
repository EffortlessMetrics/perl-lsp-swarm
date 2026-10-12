#!/usr/bin/env python3
"""Discriminating selection, stale-input and coverage controls for issue17613."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("experiment", Path(__file__).with_name("classifier_cache_experiment.py"))
experiment = importlib.util.module_from_spec(spec)
spec.loader.exec_module(experiment)


class OverlayControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.root, self.output, self.snapshot = [self.home / p for p in ["repo", "output", "overlay"]]
        self.root.mkdir()
        self.output.mkdir()
        self.environment = patch.dict(os.environ, {"CARGO_INCREMENTAL": "0", "RUST_BACKTRACE": "1",
            "FEATURES_TOML_OVERRIDE": "", "RUSTFLAGS": "", "CARGO_ENCODED_RUSTFLAGS": "",
            "RUSTC": "", "RUSTDOC": "", "RUSTC_WRAPPER": "", "RUSTC_WORKSPACE_WRAPPER": ""})
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.git("init", "-q", "-b", "main")
        self.file(".gitignore", "target/\n")
        self.file(".cargo/config.toml", "[build]\nrustflags=[]\n")
        self.file("Cargo.lock", "version=4\n")
        self.file("crates/demo/Cargo.toml", '[package]\nname="demo"\nversion="0.1.0"\n')
        self.file("crates/demo/src/lib.rs", "pub fn source() {}\n")
        self.commit()
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        self.dependency = {"id": 1, "key": experiment.DEPENDENCY_KEY, "version": "one", "size_in_bytes": 10, "ref": "refs/heads/main"}
        self.tools = {"rustc": "rustc 1.95.0 exact", "cargo": "cargo 1.95.0 exact"}
        self.binding = experiment.identity(self.root, self.dependency, self.tools)
        self.library_hash, self.runner_hash = "a" * 16, "b" * 16
        library = self.file("target/debug/deps/libdemo-" + self.library_hash + ".rlib", "library")
        self.file("target/debug/deps/demo-" + self.library_hash + ".d", "source dep-info")
        hashed = self.file("target/debug/deps/xtask-" + self.runner_hash, "runner")
        self.file("target/debug/deps/xtask-" + self.runner_hash + ".d", "runner dep-info")
        normal = self.root / "target/debug/xtask"
        os.link(hashed, normal)
        self.unit("demo", self.library_hash, "lib-demo")
        self.unit("xtask", self.runner_hash, "bin-xtask")
        self.rows = [self.row("demo", ["lib"], "demo", [library]), self.row("xtask", ["bin"], "xtask", [normal])]
        self.stderr = "fingerprint at: target/debug/.fingerprint/xtask-" + self.runner_hash + "/bin-xtask\n"
        self.stderr += "Running `rustc --crate-name xtask --crate-type bin -C extra-filename=-" + self.runner_hash + " --out-dir target/debug/deps`"
        self.file("xtask/Cargo.toml", "", tracked=False)
        # The fake xtask manifest is metadata-only and ignored by this fixture.
        self.git("add", "xtask/Cargo.toml")
        self.commit()
        self.binding = experiment.identity(self.root, self.dependency, self.tools)

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.root), *args], text=True, stderr=subprocess.DEVNULL).strip()

    def commit(self):
        self.git("add", ".")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-q", "-m", "fixture")

    def file(self, relative, data="file", tracked=True):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(data)
        return path

    def row(self, package, kind, name, files):
        manifest = self.root / ("xtask/Cargo.toml" if package == "xtask" else "crates/" + package + "/Cargo.toml")
        return {"reason": "compiler-artifact", "package_id": "path+file:///fixture/" + package + "#0.1.0",
                "manifest_path": str(manifest), "profile": {"test": False},
                "target": {"kind": kind, "name": name}, "filenames": [str(p) for p in files]}

    def unit(self, package, unit_hash, tag):
        directory = "target/debug/.fingerprint/" + package + "-" + unit_hash + "/"
        for name in [tag, tag + ".json"]:
            self.file(directory + name, "{}")
        if not tag.startswith("run-build-script-"):
            self.file(directory + "dep-" + tag)
            self.file(directory + "invoked.timestamp")

    def capture(self):
        (self.output / "cold.cargo.jsonl").write_text("\n".join(json.dumps(row) for row in self.rows))
        (self.output / "cold.cargo.stderr").write_text(self.stderr)
        experiment.capture(self.root, self.output, self.snapshot, self.binding)
        return experiment.digest((self.snapshot / "manifest.json").read_bytes())

    def test_selected_closure_preserves_alias_paths_and_excludes_other_hashes(self):
        self.file("target/debug/unselected-helper")
        self.unit("demo", "f" * 16, "lib-demo")
        sha = self.capture()
        value = experiment.validate(self.snapshot, self.binding, sha)
        paths = {p["path"] for p in value["files"]}
        self.assertIn("target/debug/xtask", paths)
        self.assertIn("target/debug/deps/xtask-" + self.runner_hash, paths)
        self.assertNotIn("target/debug/unselected-helper", paths)
        self.assertFalse(any("f" * 16 in p for p in paths))
        destination = self.home / "restored"
        destination.mkdir()
        experiment.install(self.snapshot, destination, value)
        self.assertEqual((destination / "target/debug/xtask").read_text(), "runner")

    def test_helper_and_test_metadata_are_rejected(self):
        for target, profile in [({"kind": ["bin"], "name": "unselected-helper"}, {"test": False}),
                                ({"kind": ["lib"], "name": "demo"}, {"test": True}),
                                ({"kind": ["example"], "name": "demo"}, {"test": False})]:
            with self.subTest(target=target, profile=profile):
                row = copy.deepcopy(self.rows[0])
                row["target"], row["profile"] = target, profile
                with self.assertRaises(ValueError):
                    experiment.selection(self.root, [row, self.rows[1]], self.stderr)

    def test_missing_translated_dep_info_is_visible(self):
        (self.root / ("target/debug/.fingerprint/demo-" + self.library_hash + "/dep-lib-demo")).unlink()
        with self.assertRaises(OSError):
            self.capture()

    def test_unhashed_runner_uses_observed_fingerprint_and_output_directory(self):
        (self.root / ("target/debug/deps/xtask-" + self.runner_hash)).rename(self.root / "target/debug/deps/xtask")
        (self.root / ("target/debug/deps/xtask-" + self.runner_hash + ".d")).rename(self.root / "target/debug/deps/xtask.d")
        self.stderr = "fingerprint at: target/debug/.fingerprint/xtask-" + self.runner_hash + "/bin-xtask\n"
        self.stderr += "Running `rustc --crate-name xtask --crate-type bin -C metadata=" + "e" * 16 + " --out-dir target/debug/deps`"
        sha = self.capture()
        value = experiment.validate(self.snapshot, self.binding, sha)
        self.assertIn("target/debug/deps/xtask", {r["path"] for r in value["files"]})

    def test_generated_build_output_is_captured_and_missing_file_rejected(self):
        compiler_hash, run_hash = "c" * 16, "d" * 16
        compiled = self.file("target/debug/build/demo-" + compiler_hash + "/build-script-build")
        self.unit("demo", compiler_hash, "build-script-build-script-build")
        self.unit("demo", run_hash, "run-build-script-build-script-build")
        parent = "target/debug/build/demo-" + run_hash + "/"
        for name in ["invoked.timestamp", "output", "root-output", "stderr"]:
            self.file(parent + name)
        generated = self.file(parent + "out/feature_contracts.rs", "generated")
        self.rows += [self.row("demo", ["custom-build"], "build-script-build", [compiled]),
                      {"reason": "build-script-executed", "package_id": self.rows[0]["package_id"], "out_dir": str(generated.parent)}]
        sha = self.capture()
        value = experiment.validate(self.snapshot, self.binding, sha)
        paths = {r["path"] for r in value["files"]}
        self.assertTrue(all(parent + n in paths for n in ["invoked.timestamp", "output", "root-output", "stderr"]))
        (self.snapshot / "files" / generated.relative_to(self.root)).unlink()
        with self.assertRaisesRegex(ValueError, "missing or extra"):
            experiment.validate(self.snapshot, self.binding, sha)

    def test_current_branch_dependency_shadow_and_partial_metadata_rejected(self):
        main = dict(self.dependency)
        shadow = dict(main, id=2, ref="refs/heads/experiment")
        with self.assertRaisesRegex(ValueError, "shadows"):
            experiment.dependency_record({"total_count": 2, "actions_caches": [main, shadow]}, shadow["ref"])
        with self.assertRaisesRegex(ValueError, "complete"):
            experiment.dependency_record({"total_count": 2, "actions_caches": [main]})
        self.assertEqual(experiment.dependency_record({"actions_caches": [main]}), main)

    def test_same_head_source_edit_rejects_before_install(self):
        self.capture()
        head = self.git("rev-parse", "HEAD")
        self.file("crates/demo/src/lib.rs", "pub fn changed() {}\n")
        self.assertEqual(head, self.git("rev-parse", "HEAD"))
        with self.assertRaisesRegex(ValueError, "dirty"):
            experiment.identity(self.root, self.dependency, self.tools)

    def test_toolchain_config_dependency_and_source_binding_rejected(self):
        sha = self.capture()
        for key in ["tools", "config", "dependency", "head", "recipe", "incremental"]:
            altered = copy.deepcopy(self.binding)
            altered[key] = {"changed": "value"} if isinstance(altered[key], dict) else "changed"
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "binding mismatch"):
                experiment.validate(self.snapshot, altered, sha)

    def test_actual_committed_config_change_rejects_old_snapshot(self):
        sha = self.capture()
        self.file(".cargo/config.toml", '[build]\nrustflags=["-C","opt-level=0"]\n')
        self.commit()
        current = experiment.identity(self.root, self.dependency, self.tools)
        self.assertNotEqual(current["config"], self.binding["config"])
        with self.assertRaisesRegex(ValueError, "binding mismatch"):
            experiment.validate(self.snapshot, current, sha)

    def test_override_and_profile_environment_reject_before_install(self):
        for key in ["FEATURES_TOML_OVERRIDE", "RUSTFLAGS", "CARGO_PROFILE_DEV_DEBUG"]:
            with self.subTest(key=key), patch.dict(os.environ, {key: "changed"}), self.assertRaisesRegex(ValueError, "override"):
                experiment.identity(self.root, self.dependency, self.tools)

    def test_manifest_or_file_corruption_rejected(self):
        sha = self.capture()
        with self.assertRaisesRegex(ValueError, "producer"):
            experiment.validate(self.snapshot, self.binding, "wrong")
        (self.snapshot / "files/target/debug/xtask").write_text("stale runner")
        with self.assertRaisesRegex(ValueError, "corrupt"):
            experiment.validate(self.snapshot, self.binding, sha)

    def test_path_escape_and_symlink_are_rejected(self):
        with self.assertRaises(ValueError):
            experiment.safe_file(self.home, "../outside")
        path = self.home / "link"
        try:
            path.symlink_to(self.root / "Cargo.lock")
        except OSError:
            self.skipTest("host does not allow symlink creation")
        with self.assertRaises(ValueError):
            experiment.safe_file(self.home, "link")

    def test_scope_and_subject_fail_closed_controls(self):
        subject = {"schema_version": "ci-subject.v1", "status": "resolved", "head_sha": self.binding["head"], "base_sha": self.binding["base"]}
        scope = {"schema_version": 2, "head_sha": self.binding["head"], "platform_overrides": {"windows_runner": False}}
        experiment.check_receipts(subject, scope, self.binding)
        empty = copy.deepcopy(scope)
        empty["platform_overrides"]["windows_test_crates"] = []
        experiment.check_receipts(subject, empty, self.binding)
        # serde skips an empty crate vector; production jq also accepts omission.
        missing = copy.deepcopy(scope)
        missing["platform_overrides"].pop("windows_runner")
        with self.assertRaises(ValueError):
            experiment.check_receipts(subject, missing, self.binding)
        for value in [None, "true", 1]:
            altered = copy.deepcopy(scope)
            altered["platform_overrides"]["windows_runner"] = value
            with self.assertRaises(ValueError):
                experiment.check_receipts(subject, altered, self.binding)
        for field, value in [("schema_version", 1), ("head_sha", "other")]:
            altered = copy.deepcopy(scope)
            altered[field] = value
            with self.assertRaises(ValueError):
                experiment.check_receipts(subject, altered, self.binding)
        altered = copy.deepcopy(scope)
        altered["platform_overrides"]["windows_test_crates"] = ["-p injected", None]
        with self.assertRaises(ValueError):
            experiment.check_receipts(subject, altered, self.binding)
        altered["platform_overrides"]["windows_test_crates"] = None
        with self.assertRaises(ValueError):
            experiment.check_receipts(subject, altered, self.binding)


if __name__ == "__main__":
    unittest.main()
