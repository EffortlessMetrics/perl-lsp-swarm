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
from contextlib import contextmanager

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
        proc = subprocess.run(["git", "-C", str(self.repo), *args], env=adapter.git_environment(),
                              capture_output=True, check=True)
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
        original_flag = os.environ.get("GIT_NO_REPLACE_OBJECTS")
        ledger, primitive = self.evidence()
        self.git("replace", "--graft", self.target, self.boundary)
        packet = self.check(ledger, primitive)
        self.assertEqual(packet["verdict"], "pass", packet["errors"])
        self.assertEqual(len(packet["population"]), 2)
        self.assertEqual(os.environ.get("GIT_NO_REPLACE_OBJECTS"), original_flag)

    def test_inherited_repository_overrides_cannot_redirect_git(self):
        ledger, primitive = self.evidence()
        foreign = str(self.root / "unrelated-missing-repository")
        overrides = {key: foreign for key in adapter.GIT_LOCATION_ENV}
        with patch.dict(os.environ, overrides):
            self.assertEqual(self.git("rev-parse", "--show-toplevel"), self.repo.as_posix())
            packet = self.check(ledger, primitive)
            self.assertEqual(packet["verdict"], "pass", packet["errors"])
            self.assertEqual({key: os.environ[key] for key in overrides}, overrides)

    def test_empty_cherry_still_validates_native_receipt(self):
        self.subjects["target"] = self.shared
        ledger, primitive = self.evidence()
        self.assertEqual(adapter.cherry(self.api, self.subjects), {})
        self.assertEqual(self.check(ledger, primitive)["verdict"], "pass")
        primitive["schema_version"] = 99
        packet = self.check(ledger, primitive)
        self.assertIn("foreign sync-divergence", " ".join(packet["errors"]))
        self.assertEqual(packet["verdict"], "not_proven")

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
        packet = self.check(ledger, primitive)
        self.assertEqual(packet["verdict"], "blocked", packet["errors"])
        self.assertEqual(packet["unresolved_commits"], [self.subjects["target"]])

    def test_merge_entry_resolution_and_temporary_cache_are_preserved(self):
        self.git("checkout", "-q", "-b", "side")
        self.write("shared.txt", "side behavior\n")
        self.commit("side changes an existing path")
        self.git("checkout", "-q", "public")
        self.git("merge", "--no-ff", "--no-commit", "side")
        self.write("shared.txt", "additional resolution on the same path\n")
        self.subjects["target"] = self.commit("merge with altered entry")
        ledger, primitive = self.evidence()
        packet = self.check(ledger, primitive)
        merge = next(r for r in packet["population"] if r["commit"] == self.subjects["target"])
        self.assertEqual(merge["merge_effects"]["additional_resolution_paths"], [])
        self.assertEqual(merge["merge_effects"]["entry_differences_from_side_head"], ["shared.txt"])
        self.assertEqual(packet["verdict"], "blocked", packet["errors"])
        self.assertEqual(packet["unresolved_commits"], [self.subjects["target"]])
        self.assertEqual(set(self.api.current_trees), {self.subjects["source"], self.subjects["target"]})

    def resolution_fixture(self, retain=False):
        self.git("checkout", "-q", "-b", "side")
        self.write("side.txt", "side work\n")
        self.commit("side work")
        self.git("checkout", "-q", "public")
        self.git("merge", "--no-ff", "--no-commit", "side")
        self.write("merge-only.rs", "fn resolved_behavior() {}\n")
        merge = self.commit("merge with product resolution work")
        self.subjects["target"] = merge
        if retain:
            self.git("checkout", "-q", "source")
            self.git("merge", "--no-ff", "-m", "retain original public resolution", "public")
            self.subjects["source"] = self.git("rev-parse", "HEAD")
        ledger, primitive = self.evidence()
        row = next(r for r in ledger["entries"] if r["commit"] == merge)
        effect = row["merge_resolution_dispositions"][0]
        effect.update(disposition="already_equivalent_in_swarm", source_commit=merge,
                      authority=["synthetic reviewed original resolution survives in exact S"], blocking_decisions=[])
        return ledger, primitive, row, effect

    def test_retained_original_merge_resolution_is_accepted(self):
        ledger, primitive, _, _ = self.resolution_fixture(retain=True)
        packet = self.check(ledger, primitive)
        self.assertEqual(packet["verdict"], "pass", packet["errors"])
        self.assertIn("no_product_execution", packet["acceptance_ceiling"])

    def test_reverted_original_merge_resolution_is_rejected(self):
        self.resolution_fixture(retain=True)
        self.write("merge-only.rs", "fn regressed_behavior() {}\n")
        self.subjects["source"] = self.commit("displace original resolution")
        ledger, primitive = self.evidence()
        row = next(r for r in ledger["entries"] if r["commit"] == self.subjects["target"])
        effect = row["merge_resolution_dispositions"][0]
        effect.update(disposition="already_equivalent_in_swarm", source_commit=self.subjects["target"],
                      authority=["synthetic stale survival claim"], blocking_decisions=[])
        self.assertIn("displaced", " ".join(self.check(ledger, primitive)["errors"]))

    def test_merge_resolution_requires_landed_actual_port(self):
        self.resolution_fixture()
        self.git("checkout", "-q", "source")
        self.write("merge-only.rs", "fn adapted_behavior() {}\n")
        port = self.commit("adapt resolution behavior")
        self.subjects["source"] = port
        ledger, primitive = self.evidence()
        row = next(r for r in ledger["entries"] if r["commit"] == self.subjects["target"])
        effect = row["merge_resolution_dispositions"][0]
        effect.update(disposition="port_to_swarm", source_commit=port,
                      authority=["synthetic reviewed adaptation"], blocking_decisions=[])
        self.assertEqual(self.check(ledger, primitive)["verdict"], "pass")
        effect["source_commit"] = self.source
        self.assertIn("did not change", " ".join(self.check(ledger, primitive)["errors"]))
        effect["source_commit"] = self.subjects["target"]
        self.assertIn("not reachable", " ".join(self.check(ledger, primitive)["errors"]))

    def test_merge_resolution_denominator_is_exact(self):
        ledger, primitive, row, effect = self.resolution_fixture(retain=True)
        for effects in ([], [effect, copy.deepcopy(effect)], [dict(effect, path="unrelated.rs")],
                        [dict(effect, path=[])], [dict(effect, extra="foreign field")]):
            with self.subTest(effects=effects):
                altered = copy.deepcopy(ledger)
                changed = next(r for r in altered["entries"] if r["commit"] == row["commit"])
                changed["merge_resolution_dispositions"] = effects
                self.assertEqual(self.check(altered, primitive)["verdict"], "not_proven")

    def test_merge_resolution_cannot_hide_product_as_public_context(self):
        ledger, primitive, _, effect = self.resolution_fixture(retain=True)
        effect["disposition"] = "publication_lineage_only"
        self.assertIn("lineage-only", " ".join(self.check(ledger, primitive)["errors"]))
        effect["disposition"] = "publication_context_translation"
        self.assertIn("disguised", " ".join(self.check(ledger, primitive)["errors"]))
        effect["disposition"] = "merge_ancestry"
        self.assertIn("unsupported", " ".join(self.check(ledger, primitive)["errors"]))

    def test_legacy_ledger_keeps_closed_merge_resolution_refusal(self):
        ledger, primitive, _, _ = self.resolution_fixture(retain=True)
        ledger["schema_version"] = adapter.LEGACY_LEDGER
        for row in ledger["entries"]:
            del row["merge_resolution_dispositions"]
        self.assertIn("legacy ledger cannot adjudicate", " ".join(self.check(ledger, primitive)["errors"]))

    def test_legacy_ordinary_ledger_remains_readable(self):
        ledger, primitive = self.evidence()
        ledger["schema_version"] = adapter.LEGACY_LEDGER
        for row in ledger["entries"]:
            del row["merge_resolution_dispositions"]
        self.assertEqual(self.check(ledger, primitive)["verdict"], "pass")

    def test_nonmerge_cannot_carry_resolution_dispositions(self):
        ledger, primitive = self.evidence()
        ledger["entries"][0]["merge_resolution_dispositions"] = [{"path": "shared.txt"}]
        self.assertIn("non-merge work", " ".join(self.check(ledger, primitive)["errors"]))

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

    def test_failed_stream_construction_releases_transferred_descriptor(self):
        destination = self.root / "failed-stream.json"
        descriptors = []
        def fail_stream(descriptor, *args, **kwargs):
            descriptors.append(descriptor)
            raise OSError("injected stream construction failure")
        with patch.object(adapter.os, "fdopen", fail_stream), self.assertRaises(OSError):
            adapter.write_new(self.api, destination, {}, [])
        self.assertEqual(len(descriptors), 1)
        with self.assertRaises(OSError):
            os.fstat(descriptors[0])
        # On Windows a leaked handle would also prevent this owned rename.
        moved = self.root / "closed-stream.json"
        destination.rename(moved)
        moved.unlink()

    @unittest.skipUnless(os.name == "nt", "Windows junction discriminator")
    def test_junction_inserted_after_validation_cannot_redirect_creation(self):
        parent = self.root / "raced-output"
        parent.mkdir()
        destination = parent / "receipt.json"
        original_output = adapter.exclusive_output
        inserted = False
        @contextmanager
        def race_output(current):
            nonlocal inserted
            os.rmdir(parent)
            proc = subprocess.run(["cmd", "/c", "mklink", "/J", str(parent), str(self.repo / ".git")],
                                  capture_output=True, check=True)
            inserted = True
            self.assertEqual(proc.returncode, 0)
            with original_output(current) as output:
                yield output
        try:
            with patch.object(adapter, "exclusive_output", race_output), self.assertRaises((ValueError, OSError)):
                adapter.write_new(self.api, destination, {}, [])
            self.assertTrue(inserted)
            self.assertFalse((self.repo / ".git/receipt.json").exists())
        finally:
            # Remove only this owned junction, never recurse into its target.
            if parent.is_junction():
                os.rmdir(parent)

    @unittest.skipUnless(os.name == "nt", "Windows path aliases")
    def test_windows_stream_device_and_ambiguous_paths_are_refused(self):
        for name in (".git:receipt", "NUL.json", "COM1", "receipt. ", "receipt."):
            with self.subTest(name=name), self.assertRaises(ValueError):
                adapter.write_new(self.api, self.repo / name, {}, [])

    def test_linked_pointer_and_common_git_metadata_are_refused(self):
        linked = self.root / "linked"
        self.git("worktree", "add", "-q", "-b", "linked", str(linked))
        api = adapter.Git(linked)
        common = Path(api.text("rev-parse", "--path-format=absolute", "--git-common-dir"))
        for path in (linked / ".git", common / "refs/heads/new-receipt", common / "objects/new-receipt"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                adapter.write_new(api, path, {}, [])
        (linked / "receipts").mkdir()
        adapter.write_new(api, linked / "receipts/ordinary.json", {}, [])


if __name__ == "__main__":
    unittest.main()
