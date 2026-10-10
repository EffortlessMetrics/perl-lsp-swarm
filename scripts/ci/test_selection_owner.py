"""First-slice #17485: actual shared selection owner and bootstrap contract."""
import pathlib
import copy
import json
import os
from check_selection_graph import selection_graph
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class SelectionOwner(unittest.TestCase):
    def test_two_xtask_front_doors_delegate_to_shared_owner(self):
        for name in ("ci_scope", "ci_subject"):
            wrapper = (ROOT / f"xtask/src/tasks/{name}.rs").read_text()
            self.assertIn(f"pub use perl_ci_hygiene::{name}::*;", wrapper)
            self.assertIn(f"perl_ci_hygiene::{name}::run(config)", wrapper)
            self.assertIn("crate::utils::project_root()?", wrapper)
            self.assertLess(len(wrapper.splitlines()), 30)
            shared = (ROOT / f"crates/perl-ci-hygiene/src/{name}.rs").read_text()
            self.assertNotIn("crate::tasks::", shared)
            self.assertNotIn("crate::utils::", shared)

    def test_real_platform_consumer_uses_one_exact_lightweight_binary(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        job = workflow.split("\n  platform-overrides:", 1)[1].split("\n  windows-platform-smoke:", 1)[0]
        self.assertNotIn("cargo run -p xtask", job)
        self.assertEqual(job.count("scripts/ci/bootstrap-selection.sh"), 1)
        self.assertIn('scripts/ci/check_selection_graph.py "$RUNNER_TEMP/selection-metadata.json"', job)
        self.assertIn('SELECTION_METADATA="$RUNNER_TEMP/selection-metadata.json"', job)
        for command in ("ci-subject", "ci-scope"):
            self.assertIn(f'"$selection_runner" {command}', job)
        self.assertIn('--subject "$RUNNER_TEMP/ci-subject.json"', job)
        self.assertIn('schema_version" != "2"', job)

    def test_bootstrap_uses_admitted_artifact_identity(self):
        bootstrap = (ROOT / "scripts/ci/bootstrap-selection.sh").read_text()
        self.assertIn('scripts/cargo-admitted', bootstrap)
        self.assertIn('--message-format=json', bootstrap)
        self.assertIn('compiler-artifact', bootstrap)
        self.assertIn('build-finished', bootstrap)
        self.assertIn('src_path', bootstrap)
        self.assertNotIn('target/debug', bootstrap)


class SelectionGraph(unittest.TestCase):
    def test_transitive_product_edge_is_detected(self):
        # Use Cargo's actual resolved graph when supplied; no invented package IDs.
        path = os.environ.get("SELECTION_METADATA")
        if not path:
            self.skipTest("set SELECTION_METADATA to resolved cargo metadata")
        metadata = json.loads(pathlib.Path(path).read_text())
        selection_graph(metadata)
        poisoned = copy.deepcopy(metadata)
        packages = {p["name"]: p["id"] for p in poisoned["packages"]}
        root = next(n for n in poisoned["resolve"]["nodes"] if n["id"] == packages["perl-ci-hygiene"])
        neutral = root["deps"][0]["pkg"]
        transit = next(n for n in poisoned["resolve"]["nodes"] if n["id"] == neutral)
        transit["deps"].append({"pkg": packages["perl-parser"], "dep_kinds": [{"kind": None}]})
        with self.assertRaisesRegex(ValueError, "perl-parser"):
            selection_graph(poisoned)
        # A product dev-only edge does not change the executable graph.
        transit["deps"][-1]["dep_kinds"][0]["kind"] = "dev"
        selection_graph(poisoned)


if __name__ == "__main__":
    unittest.main()
