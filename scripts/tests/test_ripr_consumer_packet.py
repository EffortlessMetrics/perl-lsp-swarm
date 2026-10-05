#!/usr/bin/env python3
"""Offline packet controls, using native Git as the independent tree oracle."""

import contextlib
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("ripr_packet", ROOT / "scripts/ci/prepare_ripr_consumer_packet.py")
PACKET = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKET)


class ConsumerPacket(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.repo = Path(cls.temp.name) / "repo"
        cls.repo.mkdir()
        cls.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        cls.env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
        cls.git("init", "-b", "main")
        cls.git("config", "user.name", "Fixture")
        cls.git("config", "user.email", "fixture@example.invalid")
        # Directory/file prefix and Unicode paths discriminate Git's byte ordering.
        files = {"a.rs": b"base rust\n", "a/\u03bb.rs": b"nested rust\n", "Cargo.toml": b"fixture config\n",
                 ".github/workflows/ripr.yml": b"frozen workflow fixture\n",
                 "xtask/src/tasks/ripr_evidence.rs": b"producer fixture\n",
                 "xtask/src/tasks/quality_gate.rs": b"gate fixture\n"}
        for name, raw in files.items():
            path = cls.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(raw)
        cls.git("add", ".")
        cls.git("commit", "-m", "initial fixture")
        cls.git("checkout", "-b", "feature")
        (cls.repo / "a.rs").write_bytes(b"changed rust\n")
        (cls.repo / "new.rs").write_bytes(b"new rust\n")
        cls.git("add", ".")
        cls.git("commit", "-m", "feature")
        cls.head = cls.git("rev-parse", "HEAD").decode().strip()
        cls.git("checkout", "main")
        (cls.repo / "main.txt").write_bytes(b"independent base advance\n")
        cls.git("add", ".")
        cls.git("commit", "-m", "base")
        cls.base = cls.git("rev-parse", "HEAD").decode().strip()
        cls.git("merge", "--no-ff", "feature", "-m", "evaluated merge")
        cls.evaluated = cls.git("rev-parse", "HEAD").decode().strip()
        cls.tree = cls.git("rev-parse", "HEAD^{tree}").decode().strip()
        cls.e_snapshot = cls.snapshot(cls.evaluated)
        cls.b_snapshot = cls.snapshot(cls.base)
        cls.e_path = Path(cls.temp.name) / "evaluated.json"
        cls.b_path = Path(cls.temp.name) / "base.json"
        cls.e_path.write_text(json.dumps(cls.e_snapshot), encoding="utf-8")
        cls.b_path.write_text(json.dumps(cls.b_snapshot), encoding="utf-8")
        cls.profile = {name: cls.git("rev-parse", "HEAD:" + name).decode().strip()
                       for name in PACKET.KNOWN_BLOBS}

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    @classmethod
    def git(cls, *args):
        return subprocess.check_output(["git", "-C", str(cls.repo), *args], env=cls.env,
                                       stderr=subprocess.PIPE, timeout=10)

    @classmethod
    def snapshot(cls, revision):
        # Native Git emits all paths/modes/types/IDs, independent of the collector.
        entries = []
        for row in cls.git("ls-tree", "-r", "-t", "-z", revision).split(b"\0"):
            if not row:
                continue
            metadata, path = row.split(b"\t", 1)
            mode, kind, identity = metadata.decode().split()
            entry = {"path": path.decode("utf-8"), "mode": mode, "type": kind, "sha": identity}
            if kind == "blob":
                entry["size"] = int(cls.git("cat-file", "-s", identity))
            entries.append(entry)
        return {"sha": cls.git("rev-parse", revision + "^{tree}").decode().strip(),
                "truncated": False, "tree": entries}

    def collect(self, **overrides):
        args = dict(repo=self.repo, evaluated=self.evaluated, base=self.base, head=self.head,
                    expected_tree=self.tree, evaluated_snapshot=self.e_path, base_snapshot=self.b_path)
        args.update(overrides)
        # Toy source bodies test collection mechanics, not the real analyzer recipe.
        # The real frozen profile is exercised by a separately retained native packet.
        with patch.dict(PACKET.KNOWN_BLOBS, self.profile, clear=True):
            return PACKET.collect(**args)

    def test_complete_packet_is_nonadmitting_and_preserves_unknowns(self):
        packet = self.collect()
        self.assertEqual(packet["subject"]["ordered_parents"], [self.base, self.head])
        self.assertNotEqual(self.head, self.evaluated)
        self.assertEqual(packet["admission_effect"], "none")
        self.assertEqual(packet["qualification"], "NOT_PROVEN")
        self.assertEqual(set(packet["required_runtime_evidence"].values()), {"NOT_PROVEN"})
        self.assertEqual(packet["evaluated_inventory"]["tracked_regular_rust_candidates"], 5)
        self.assertEqual([(row["path"], row["change"]) for row in packet["changed_paths"]],
                         [("a.rs", "modified"), ("new.rs", "added")])
        self.assertEqual([row["path"] for row in packet["named_configuration_group"]], ["Cargo.toml"])
        self.assertIn("API-reported", packet["snapshots"]["byte_counts"])

    def test_full_sequence_has_four_baseline_analyses_and_exports(self):
        phases = {row["phase"]: row for row in self.collect()["proof_sequence"]}
        formats = ["repo-badge-json", "repo-seams-json"]
        self.assertEqual(phases["baseline"]["fresh_generation_repo_checks"], formats)
        self.assertEqual(phases["validation"]["additional_repo_checks"], formats)
        self.assertIn("hosted", self.collect()["source_profile"]["lane"])
        self.assertIn("cargo install ripr --version 0.10.0 --locked", phases["freshness_and_build"]["host_setup"])
        self.assertIn("cargo install ripr --version 0.10.0 --locked", phases["pr_evidence"]["container_setup"])
        self.assertIn("observe-xtask-build.sh container -- cargo build -p xtask --locked",
                      phases["pr_evidence"]["container_setup"])
        self.assertIn("ripr doctor", phases["pr_evidence"]["container_setup"])
        self.assertIn("25m", phases["pr_evidence"]["analysis_envelope"])
        self.assertIn("40m", phases["pr_evidence"]["container_envelope"])
        self.assertIn("no new RIPR analysis", phases["pr_evidence"]["immediate_validation"][1])
        self.assertIn("no new RIPR analysis", phases["validation"]["pr_check"])
        self.assertIn("--check", phases["genuine_gap_gate"]["commands"][1])
        self.assertIn("ready", phases["proof_export"]["condition"])
        self.assertIn("partial diagnostics", phases["proof_export"]["proof_boundary"])
        self.assertIn("static-limitation credit", phases["genuine_gap_gate"]["requires"])
        self.assertIn("no proof authority", phases["diagnostic_export"]["condition"])
        self.assertIn("success()", phases["memo_save"]["condition"])

    def test_native_git_roster_with_unicode_and_prefix_order(self):
        leaves = PACKET.verify_tree(self.e_snapshot, self.tree)
        self.assertIn("a/\u03bb.rs", leaves)
        self.assertEqual(len(leaves), 8)

    def test_omitted_entry_with_false_truncation_flag_is_rejected(self):
        bad = copy.deepcopy(self.e_snapshot)
        bad["tree"] = [entry for entry in bad["tree"] if entry["path"] != "a.rs"]
        with self.assertRaisesRegex(PACKET.PacketError, "reconstruction mismatch"):
            PACKET.verify_tree(bad, self.tree)

    def test_tree_snapshot_integrity_controls(self):
        for variant in ("truncated", "wrong_root", "duplicate", "missing_parent", "mode", "empty", "bad_mode"):
            with self.subTest(variant=variant):
                bad = copy.deepcopy(self.e_snapshot)
                if variant == "truncated":
                    bad["truncated"] = True
                elif variant == "wrong_root":
                    bad["sha"] = "0" * 40
                elif variant == "duplicate":
                    bad["tree"].append(copy.deepcopy(bad["tree"][0]))
                elif variant == "missing_parent":
                    bad["tree"] = [entry for entry in bad["tree"] if entry["path"] != "a"]
                elif variant in {"mode", "bad_mode"}:
                    next(entry for entry in bad["tree"] if entry["path"] == "a.rs")["mode"] = (
                        "100755" if variant == "mode" else [])
                else:
                    bad["tree"] = []
                with self.assertRaises(PACKET.PacketError):
                    PACKET.verify_tree(bad, self.tree)

    def test_expected_subject_is_independent_of_snapshots(self):
        for fields in ({"evaluated": self.head}, {"base": self.head}, {"head": self.base},
                       {"expected_tree": self.b_snapshot["sha"]}):
            with self.subTest(fields=fields), self.assertRaises(PACKET.PacketError):
                self.collect(**fields)

    def test_wrong_consumer_blob_is_refused(self):
        with self.assertRaisesRegex(PACKET.PacketError, "unsupported consumer"):
            PACKET.collect(self.repo, self.evaluated, self.base, self.head, self.tree, self.e_path, self.b_path)

    def test_workflow_object_content_is_verified(self):
        original = PACKET.read_object
        def wrong(repo, kind, identity):
            if kind == "blob":
                raise PACKET.PacketError("wrong Git object type or content")
            return original(repo, kind, identity)
        with patch.object(PACKET, "read_object", side_effect=wrong), self.assertRaises(PACKET.PacketError):
            self.collect()

    def test_api_size_is_not_claimed_as_git_verified(self):
        bad = copy.deepcopy(self.e_snapshot)
        next(entry for entry in bad["tree"] if entry["path"] == "a.rs")["size"] += 50
        # Git tree identity binds paths/modes/object IDs, not this metadata field.
        self.assertEqual(PACKET.verify_tree(bad, self.tree)["a.rs"]["size"], 63)

    def test_bounded_read_and_duplicate_json_keys(self):
        path = Path(self.temp.name) / "oversized.json"
        path.write_bytes(b" " * (PACKET.MAX_TREE_BYTES + 1))
        with self.assertRaisesRegex(PACKET.PacketError, "8 MiB"):
            PACKET.read_snapshot(path)
        path.write_text('{"sha":"first","sha":"second"}', encoding="utf-8")
        with self.assertRaisesRegex(PACKET.PacketError, "duplicate JSON"):
            PACKET.read_snapshot(path)

    def test_cli_success_is_collection_only_and_failure_emits_no_packet(self):
        argv = ["--repo", str(self.repo), "--evaluated-head", self.evaluated, "--base", self.base,
                "--pr-head", self.head, "--expected-tree", self.tree,
                "--evaluated-tree-json", str(self.e_path), "--base-tree-json", str(self.b_path)]
        out, err = io.StringIO(), io.StringIO()
        with patch.dict(PACKET.KNOWN_BLOBS, self.profile, clear=True), contextlib.redirect_stdout(out):
            self.assertEqual(PACKET.main(argv), 0)
        self.assertEqual(json.loads(out.getvalue())["admission_effect"], "none")
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            self.assertEqual(PACKET.main(argv), 2)
        self.assertEqual(out.getvalue(), "")
        self.assertIn("refused", err.getvalue())


if __name__ == "__main__":
    unittest.main()
