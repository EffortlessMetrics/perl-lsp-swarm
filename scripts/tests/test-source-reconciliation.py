#!/usr/bin/env python3
"""Real-Git falsifiers for the non-release reconciliation composition."""
import copy
import hashlib
import importlib.util
import json
import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("source_reconciliation",
    Path(__file__).resolve().parents[1] / "source_reconciliation.py")
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)


class ReconciliationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="source-reconcile-")
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.git("init", "-q")
        self.git("config", "user.name", "Synthetic test")
        self.git("config", "user.email", "synthetic@example.invalid")
        self.boundary = self.commit("boundary")
        self.write("shared.txt", "original shared behavior\n")
        self.shared = self.commit("shared work")
        self.git("checkout", "-q", "-b", "source")
        self.write("source.txt", "source only\n")
        self.source = self.commit("source work")
        self.git("checkout", "-q", "-b", "public", self.shared)
        self.write("public.txt", "explicitly rejected synthetic behavior\n")
        self.target = self.commit("public work")
        self.subjects = {"source": self.source, "boundary": self.boundary, "target": self.target}
        self.api = adapter.Git(self.repo)

    def tearDown(self):
        for path in self.root.rglob("*"):
            if path.is_file():
                path.chmod(stat.S_IWRITE)
        self.temp.cleanup()

    def git(self, *args):
        proc = subprocess.run(["git", "-C", str(self.repo), *args], capture_output=True, check=True)
        return proc.stdout.decode().strip()

    def write(self, path, content):
        path = self.repo / path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    def commit(self, message):
        self.git("add", "--all")
        self.git("commit", "-q", "--allow-empty", "-m", message)
        return self.git("rev-parse", "HEAD")

    def evidence(self):
        packet = adapter.reconcile(self.api, self.subjects)
        ledger = adapter.skeleton(packet)
        patches = adapter.cherry(self.api, self.subjects)
        native_rows = []
        for item in ledger["entries"]:
            item["blocking_decisions"] = []
            item["authority"] = ["synthetic accepted semantic decision, not authenticated production evidence"]
            if item["commit"] == self.shared:
                item["disposition"] = "already_equivalent_in_swarm"
                item["source_commit"] = self.shared
            elif len(next(r for r in packet["population"] if r["commit"] == item["commit"])["parents"]) > 1:
                item["disposition"] = "merge_ancestry"
            else:
                item["disposition"] = "deliberately_abandoned"
            if patches.get(item["commit"]) == "+":
                row = next(r for r in packet["population"] if r["commit"] == item["commit"])
                native_rows.append({"commit": row["commit"], "subject": row["subject"],
                                    "classification": adapter.PRIMITIVE_MAP[item["disposition"]]})
        raw = "".join(r["commit"] + " " + " ".join(r["subject"].split()) + "\n"
                      for r in sorted(native_rows, key=lambda r: r["commit"]))
        merges = [{k: r[k] for k in ("commit", "subject", "parents")}
                  for r in packet["population"] if len(r["parents"]) > 1]
        primitive = {"schema_version": 2,
            "subjects": {n: {"role": role, "input": self.subjects[n], "commit": self.subjects[n]}
                         for n, role in adapter.ROLES.items()},
            "ledger": "synthetic-native-ledger.json", "verdict": "pass",
            "population_digest": hashlib.sha256(raw.encode()).hexdigest(),
            "target_unique_commits": native_rows,
            "excluded_merge_commits": [r["commit"] for r in merges],
            "excluded_merge_ancestry": merges, "excluded_release_lineage_commits": [],
            "accepted_commits": [r["commit"] for r in native_rows],
            "unresolved_commits": [], "errors": []}
        return ledger, primitive

    def check(self, ledger, primitive):
        return adapter.reconcile(self.api, self.subjects, ledger, primitive)

    def test_complete_population_includes_shared_work(self):
        packet = adapter.reconcile(self.api, self.subjects)
        self.assertEqual({r["commit"] for r in packet["population"]}, {self.shared, self.target})
        self.assertEqual(packet["verdict"], "not_proven")

    def test_valid_structural_preflight_has_explicit_acceptance_ceiling(self):
        ledger, primitive = self.evidence()
        packet = self.check(ledger, primitive)
        self.assertEqual(packet["verdict"], "pass", packet["errors"])
        self.assertIn("no_source_admission", packet["acceptance_ceiling"])

    def test_omitting_shared_work_cannot_use_unique_only_denominator(self):
        ledger, primitive = self.evidence()
        ledger["entries"] = [r for r in ledger["entries"] if r["commit"] != self.shared]
        self.assertIn("omitted", " ".join(self.check(ledger, primitive)["errors"]))

    def test_duplicate_work_unit_is_rejected(self):
        ledger, primitive = self.evidence()
        ledger["entries"].append(copy.deepcopy(ledger["entries"][0]))
        self.assertEqual(self.check(ledger, primitive)["verdict"], "not_proven")

    def test_reachable_equivalent_that_was_reverted_is_rejected(self):
        self.git("checkout", "-q", "source")
        self.write("shared.txt", "regressed behavior\n")
        self.subjects["source"] = self.commit("revert shared behavior")
        ledger, primitive = self.evidence()
        self.assertIn("displaced", " ".join(self.check(ledger, primitive)["errors"]))

    def test_unmerged_port_is_rejected(self):
        ledger, primitive = self.evidence()
        row = next(r for r in ledger["entries"] if r["commit"] == self.target)
        row["disposition"], row["source_commit"] = "port_to_swarm", self.target
        primitive["target_unique_commits"][0]["classification"] = "port_to_swarm"
        self.assertIn("not reachable", " ".join(self.check(ledger, primitive)["errors"]))

    def test_absence_at_reachable_source_tip_is_not_a_port(self):
        ledger, primitive = self.evidence()
        row = next(r for r in ledger["entries"] if r["commit"] == self.target)
        row["disposition"], row["source_commit"] = "port_to_swarm", self.source
        primitive["target_unique_commits"][0]["classification"] = "port_to_swarm"
        self.assertIn("did not change", " ".join(self.check(ledger, primitive)["errors"]))

    def test_actual_landed_port_and_deletion_are_accepted(self):
        self.git("checkout", "-q", "source")
        self.write("public.txt", "adapted behavior\n")
        port = self.commit("port public behavior")
        self.subjects["source"] = port
        ledger, primitive = self.evidence()
        row = next(r for r in ledger["entries"] if r["commit"] == self.target)
        row["disposition"], row["source_commit"] = "port_to_swarm", port
        primitive["target_unique_commits"][0]["classification"] = "port_to_swarm"
        self.assertEqual(self.check(ledger, primitive)["verdict"], "pass")

        self.git("checkout", "-q", "public")
        self.git("rm", "-q", "shared.txt")
        deletion = self.commit("remove shared behavior in public")
        self.subjects["target"] = deletion
        self.git("checkout", "-q", "source")
        self.git("rm", "-q", "shared.txt")
        self.subjects["source"] = self.commit("port deletion with independent history")
        ledger, primitive = self.evidence()
        # The original shared addition is explicitly superseded by this removal.
        shared = next(r for r in ledger["entries"] if r["commit"] == self.shared)
        shared["disposition"] = "superseded_by_swarm_architecture"
        shared["source_commit"] = self.subjects["source"]
        row = next(r for r in ledger["entries"] if r["commit"] == deletion)
        row["disposition"], row["source_commit"] = "port_to_swarm", self.subjects["source"]
        for item in primitive["target_unique_commits"]:
            if item["commit"] == deletion:
                item["classification"] = "port_to_swarm"
        self.assertEqual(self.check(ledger, primitive)["verdict"], "pass")

    def test_mutable_source_proof_is_rejected(self):
        ledger, primitive = self.evidence()
        ledger["entries"][0]["source_commit"] = "source"
        self.assertEqual(self.check(ledger, primitive)["verdict"], "not_proven")

    def context_evidence(self):
        self.git("checkout", "-q", "-b", "context", self.shared)
        self.write(".github/source.yml", "public context\n")
        self.subjects["target"] = self.commit("public context")
        ledger, primitive = self.evidence()
        row = next(r for r in ledger["entries"] if r["commit"] == self.subjects["target"])
        row["disposition"] = "publication_context_translation"
        row["projection_bindings"] = [{"path": ".github/source.yml", "row_id": "synthetic-context",
            "row_digest": "a" * 64, "entry": self.api.entry(self.subjects["target"], ".github/source.yml")}]
        primitive["target_unique_commits"][0]["classification"] = None
        primitive["accepted_commits"] = []
        primitive["unresolved_commits"] = [self.subjects["target"]]
        primitive["verdict"] = "blocked"
        return ledger, primitive

    def test_projection_requires_an_immutable_tree_and_binds_its_identity(self):
        ledger, primitive = self.context_evidence()
        tree = self.git("rev-parse", "HEAD^{tree}")
        first = adapter.reconcile(self.api, self.subjects, ledger, primitive, tree)
        self.assertEqual(first["verdict"], "pass", first["errors"])
        for mutable in ("HEAD", "context", self.subjects["target"]):
            packet = adapter.reconcile(self.api, self.subjects, ledger, primitive, mutable)
            self.assertIn("immutable exact tree", " ".join(packet["errors"]))
        self.write("unrelated.txt", "different projection tree\n")
        self.commit("different tree with same context path")
        second = adapter.reconcile(self.api, self.subjects, ledger, primitive, self.git("rev-parse", "HEAD^{tree}"))
        self.assertEqual(second["verdict"], "pass", second["errors"])
        self.assertNotEqual(first["packet_digest"], second["packet_digest"])
        self.write(".github/source.yml", "displaced context\n")
        self.commit("changed context entry")
        third = adapter.reconcile(self.api, self.subjects, ledger, primitive, self.git("rev-parse", "HEAD^{tree}"))
        self.assertIn("entry/mode identity", " ".join(third["errors"]))

    def test_replace_overlay_cannot_truncate_original_population(self):
        ledger, primitive = self.evidence()
        self.git("replace", "--graft", self.target, self.boundary)
        packet = self.check(ledger, primitive)
        self.assertEqual(packet["verdict"], "pass", packet["errors"])
        self.assertEqual(len(packet["population"]), 2)
        original_flag = os.environ.get("GIT_NO_REPLACE_OBJECTS")
        self.assertEqual(os.environ.get("GIT_NO_REPLACE_OBJECTS"), original_flag)

    def test_legacy_graft_file_and_environment_fail_closed(self):
        ledger, primitive = self.evidence()
        grafts = self.repo / ".git/info/grafts"
        grafts.write_text(self.target + " " + self.boundary + "\n", encoding="ascii")
        self.assertIn("legacy graft file", " ".join(self.check(ledger, primitive)["errors"]))
        grafts.unlink()
        with patch.dict(os.environ, {"GIT_GRAFT_FILE": str(self.root / "external-grafts")}):
            self.assertIn("legacy graft environment", " ".join(self.check(ledger, primitive)["errors"]))

    def test_current_path_omission_is_rejected(self):
        ledger, primitive = self.evidence()
        ledger["entries"][0]["current_bindings"] = []
        self.assertIn("omission", " ".join(self.check(ledger, primitive)["errors"]))

    def test_native_roles_and_population_are_independently_checked(self):
        ledger, primitive = self.evidence()
        primitive["subjects"]["source"]["role"] = "release_head"
        primitive["target_unique_commits"] = []
        packet = self.check(ledger, primitive)
        self.assertEqual(packet["verdict"], "not_proven")
        self.assertTrue(any("subject/role" in e for e in packet["errors"]))

    def test_native_blocked_verdict_cannot_be_forged_to_pass(self):
        ledger, primitive = self.evidence()
        primitive["unresolved_commits"] = [self.target]
        self.assertEqual(self.check(ledger, primitive)["verdict"], "not_proven")

    def test_stale_subject_is_rejected(self):
        ledger, primitive = self.evidence()
        ledger["subjects"] = dict(ledger["subjects"], target=self.shared)
        self.assertIn("stale", " ".join(self.check(ledger, primitive)["errors"]))

    def test_unknown_disposition_fails_closed(self):
        ledger, primitive = self.evidence()
        ledger["entries"][0]["disposition"] = "looks_equivalent"
        self.assertEqual(self.check(ledger, primitive)["verdict"], "not_proven")

    def test_merge_resolution_only_effect_is_not_dropped(self):
        self.git("checkout", "-q", "-b", "side")
        self.write("side.txt", "side work\n")
        self.commit("side work")
        self.git("checkout", "-q", "public")
        self.git("merge", "--no-ff", "--no-commit", "side")
        self.write("merge-only.txt", "resolution-only behavior\n")
        self.subjects["target"] = self.commit("merge with additional resolution work")
        ledger, primitive = self.evidence()
        self.assertIn("resolution effects", " ".join(self.check(ledger, primitive)["errors"]))

    def test_shallow_graph_cannot_pass(self):
        (self.repo / ".git/shallow").write_text(self.shared + "\n")
        self.assertEqual(adapter.reconcile(self.api, self.subjects)["verdict"], "not_proven")

    def test_named_control_test_does_not_exempt_other_product_tests(self):
        self.assertTrue(adapter.context_path("scripts/tests/test-publication-sync-contract.py"))
        self.assertFalse(adapter.context_path("scripts/tests/test-runtime.py"))
        self.assertFalse(adapter.context_path("xtask/src/tasks/workflow_trigger_lint.rs"))

    def test_historical_packet_is_never_overwritten(self):
        path = self.root / "historical.json"
        path.write_text("historical")
        with self.assertRaises(ValueError):
            adapter.write_new(self.api, path, {}, [])
        self.assertEqual(path.read_text(), "historical")

    def test_linked_pointer_and_common_git_metadata_are_refused(self):
        linked = self.root / "linked"
        self.git("worktree", "add", "-q", "-b", "linked", str(linked))
        api = adapter.Git(linked)
        common = Path(api.text("rev-parse", "--path-format=absolute", "--git-common-dir"))
        for path in (linked / ".git", common / "refs/heads/new-receipt", common / "objects/new-receipt"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                adapter.write_new(api, path, {}, [])
        adapter.write_new(api, linked / "receipts/ordinary.json", {}, [])


if __name__ == "__main__":
    unittest.main()
