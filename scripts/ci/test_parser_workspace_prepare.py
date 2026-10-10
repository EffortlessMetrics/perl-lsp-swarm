"""Focused controls; never compile the product or claim native runtime proof."""
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("parser_prepare", Path(__file__).with_name("parser_workspace_prepare.py"))
prepare = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(prepare)


class PreparationControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.source = self.base / "source"
        self.source.mkdir()
        self.env = self.resources("slot-a", "tree-a")

    def resources(self, slot_name, tree_name):
        slot = self.base / slot_name
        pair = slot / "worktrees" / tree_name
        target, build = pair / "target", pair / "build"
        marker = slot / "cargo-active" / "owner-fixture"
        for path in (target, build, marker):
            path.mkdir(parents=True, exist_ok=True)
        receipt = {"worktree": str(self.source), "resources": {"target": str(target), "build": str(build)},
                   "lease": str(marker.parent), "lease_marker": str(marker)}
        return {"CARGO_ADMITTED_RESOURCES": json.dumps(receipt), "CARGO_TARGET_DIR": str(target),
                "CARGO_BUILD_BUILD_DIR": str(build)}

    def execute(self, invoke, env=None):
        # The production root is fixed; fixtures change only the source subject.
        original = prepare.admitted_resources
        with patch.object(prepare, "ROOT", self.source), \
             patch.object(prepare, "admitted_resources", lambda env: original(env, self.source)), \
             patch("sys.stdout", io.StringIO()), patch("sys.stderr", io.StringIO()):
            return prepare.run(self.env if env is None else env, invoke)

    def test_exact_commands_keep_both_private_roots_through_all_consumers(self):
        calls = []
        def invoke(args, **kwargs):
            calls.append((args, kwargs))
            return subprocess.CompletedProcess(args, 0, "running 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n", "")
        self.assertEqual(self.execute(invoke), 0)
        for (actual, _), expected in zip(calls, prepare.COMMANDS):
            operation = actual.index(expected[0])
            self.assertEqual(actual[operation:], [expected[0], "--target-dir", self.env["CARGO_TARGET_DIR"], *expected[1:]])
            self.assertIn("build.build-dir=" + json.dumps(self.env["CARGO_BUILD_BUILD_DIR"]), actual)
            self.assertIn('build.rustc-wrapper=""', actual)
            self.assertIn('build.rustc-workspace-wrapper=""', actual)
        for _, kwargs in calls:
            self.assertEqual(kwargs["cwd"], self.source)
            self.assertEqual(kwargs["env"], self.env)
        self.assertNotEqual(self.env["CARGO_TARGET_DIR"], self.env["CARGO_BUILD_BUILD_DIR"])

    def test_each_failure_stops_before_subsequent_work(self):
        for failed in range(4):
            calls = []
            def invoke(args, **kwargs):
                calls.append(args)
                return subprocess.CompletedProcess(args, 101 if len(calls) == failed + 1 else 0, "", "failure")
            self.assertEqual(self.execute(invoke), 101)
            self.assertEqual(len(calls), failed + 1)

    def test_build_warnings_preserve_parser_only_anchor_scan(self):
        calls = []
        def invoke(args, **kwargs):
            calls.append(args)
            return subprocess.CompletedProcess(args, 0, "", "warning: issue\n --> crates\\perl-parser\\src\\lib.rs:1\n" if "build" in args else "")
        self.assertEqual(self.execute(invoke), 1)
        self.assertEqual(len(calls), 2)
        self.assertFalse(prepare.warnings_anchored_in_parser("warning: issue\n --> crates/other/src/lib.rs:1"))

    def test_missing_or_stale_lease_cannot_start_cargo(self):
        for env in ({}, self.env | {"CARGO_ADMITTED_RESOURCES": "null"}):
            with self.assertRaises(ValueError):
                self.execute(lambda *_args, **_kwargs: self.fail("Cargo must not start"), env)
        receipt = json.loads(self.env["CARGO_ADMITTED_RESOURCES"])
        Path(receipt["lease_marker"]).rmdir()
        with self.assertRaises(ValueError):
            self.execute(lambda *_args, **_kwargs: self.fail("Cargo must not start"))

    def test_shared_or_overlapping_intermediates_cannot_start_cargo(self):
        for variable in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR"):
            with self.assertRaises(ValueError):
                self.execute(lambda *_args, **_kwargs: self.fail("Cargo must not start"), self.env | {variable: str(self.base)})
        receipt = json.loads(self.env["CARGO_ADMITTED_RESOURCES"])
        receipt["resources"]["build"] = receipt["resources"]["target"]
        env = self.env | {"CARGO_ADMITTED_RESOURCES": json.dumps(receipt), "CARGO_BUILD_BUILD_DIR": receipt["resources"]["build"]}
        with self.assertRaises(ValueError):
            self.execute(lambda *_args, **_kwargs: self.fail("Cargo must not start"), env)

    def test_concurrent_agent_domains_cannot_mix_final_and_intermediate_roots(self):
        second = self.resources("slot-b", "tree-b")
        for env in (self.env, second):
            prepare.admitted_resources(env, self.source)
        for key in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR"):
            self.assertNotEqual(self.env[key], second[key])
            with self.assertRaises(ValueError):
                prepare.admitted_resources(self.env | {key: second[key]}, self.source)

    def test_ownership_loss_stops_before_next_command(self):
        calls = []
        def invoke(args, **kwargs):
            calls.append(args)
            Path(json.loads(self.env["CARGO_ADMITTED_RESOURCES"])["lease_marker"]).rmdir()
            return subprocess.CompletedProcess(args, 0, "", "")
        with self.assertRaises(ValueError):
            self.execute(invoke)
        self.assertEqual(len(calls), 1)

    def test_transfer_keeps_targets_and_reuses_real_runtime_evidence_guard(self):
        root = Path(__file__).resolve().parents[2]
        for name in ("issue_146_unit_tests.rs", "issue_146_architectural_integrity_tests.rs"):
            text = (root / "crates/perl-parser/tests" / name).read_text()
            self.assertIn("parser_workspace_prepare", text)
            self.assertNotIn("perl-parser-qa-nested-target", text)
        runner = (root / "xtask/src/tasks/gates.rs").read_text()
        self.assertIn("routed_preparation::RUNTIME | routed_preparation::PARSER", runner)
        self.assertIn("routed_runtime_evidence::validate", runner)


if __name__ == "__main__":
    unittest.main()
