#!/usr/bin/env python3

from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

MODULE_PATH = Path(__file__).resolve().parents[1] / "cargo_retirement.py"
SPEC = importlib.util.spec_from_file_location("cargo_retirement", MODULE_PATH)
assert SPEC and SPEC.loader
retirement = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(retirement)


def run(*args: str, cwd: Path) -> str:
    result = subprocess.run(args, cwd=cwd, text=True, capture_output=True, check=True)
    return result.stdout.strip()


class FakeAllocation:
    def __init__(self, worktree, slot, paths, env):
        self.path = (
            retirement.Path(slot)
            / "worktrees"
            / hashlib.sha256(os.fsencode(worktree)).hexdigest()
            / "allocation.json"
        )
        self.receipt = {
            "status": "matching_creation_record" if self.path.exists() else "unknown",
            "record_path": str(self.path),
            "consumer_custody": "unknown",
            "reclaim_eligibility": "unknown",
        }


class CargoRetirementTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root, ignore_errors=True)
        self.remote = self.root / "origin.git"
        self.worktree = self.root / "source"
        self.devplane = self.root / "devplane"
        subprocess.run(
            ["git", "init", "--bare", "--initial-branch=main", str(self.remote)],
            check=True,
            capture_output=True,
        )
        subprocess.run(["git", "clone", str(self.remote), str(self.worktree)], check=True, capture_output=True)
        run("git", "config", "user.name", "Fixture", cwd=self.worktree)
        run("git", "config", "user.email", "fixture@example.com", cwd=self.worktree)
        (self.worktree / "file.txt").write_text("fixture\n", encoding="utf-8")
        run("git", "add", "file.txt", cwd=self.worktree)
        run("git", "commit", "-m", "fixture", cwd=self.worktree)
        run("git", "push", "-u", "origin", "main", cwd=self.worktree)
        self.state_path = self.root / "ops" / "state.json"
        self.env = {**os.environ, "DEVPLANE": str(self.devplane)}
        self.cargo = self.fake_cargo()
        _, _, self.slot, self.paths, _, _ = retirement.resource_plan_for(
            self.worktree, self.env, cargo=self.cargo
        )
        private = self.cargo.private_worktree_root(self.slot, self.worktree.resolve())
        self.slot.mkdir(parents=True)
        private.mkdir(parents=True)
        self.paths["target"].mkdir()
        self.paths["build"].mkdir()
        self.paths["temp"].mkdir(parents=True, exist_ok=True)
        (private / "allocation.json").write_text("{}\n", encoding="utf-8")
        self.slot_record = {
            "slot_id": "slot-1",
            "path": str(self.worktree),
            "branch": "main",
            "owner": None,
            "status": "retired",
            "last_used_at": "2026-11-09T00:00:00Z",
            "last_released_at": "2026-11-09T01:00:00Z",
        }

    def fake_cargo(self):
        fake_source = self.root / "cargo_admitted.py"
        fake_source.write_text("# fixture\n", encoding="utf-8")

        def native_path(value):
            return Path(value).resolve()

        def read_only_git_environment(env):
            return dict(env)

        def git_path(*args, env=None, output_reader=None):
            output = output_reader(["git", "rev-parse", *args], env)
            return native_path(os.fsdecode(output.rstrip(b"\n")))

        def private_worktree_root(slot, worktree):
            return Path(slot) / "worktrees" / hashlib.sha256(os.fsencode(worktree)).hexdigest()

        def resource_plan(env, *, git_output_reader=None):
            worktree = git_path("--show-toplevel", env=env, output_reader=git_output_reader)
            common = git_path("--path-format=absolute", "--git-common-dir", env=env, output_reader=git_output_reader)
            identity = str(common).casefold() if os.name == "nt" else str(common)
            key = hashlib.sha256(os.fsencode(identity)).hexdigest()[:16]
            slot = native_path(env["DEVPLANE"]) / ("fixture-" + sys.platform + "-" + key)
            private = private_worktree_root(slot, worktree)
            paths = {
                "target": private / "target",
                "build": private / "build",
                "cargo_home": self.root / "cargo-home",
                "temp": slot / "tmp",
            }
            return worktree, slot, paths

        def file_subject(path):
            raw = Path(path).read_bytes()
            return {"path": str(path), "sha256": hashlib.sha256(raw).hexdigest()}

        return SimpleNamespace(
            __file__=str(fake_source),
            native_path=native_path,
            read_only_git_environment=read_only_git_environment,
            git_path=git_path,
            private_worktree_root=private_worktree_root,
            resource_plan=resource_plan,
            PrivateAllocation=FakeAllocation,
            file_subject=file_subject,
        )

    def plan(self, *, created_at="2026-11-09T02:00:00Z"):
        return retirement.plan_retirement(
            self.worktree,
            repo_root=self.worktree,
            slot_record=self.slot_record,
            state_path=self.state_path,
            cleanup_reason="retired",
            env=self.env,
            created_at=created_at,
            cargo=self.cargo,
        )

    def linked_plan(self, name: str):
        linked = self.root / name
        branch = "fixture/" + name
        run("git", "worktree", "add", "-b", branch, str(linked), "main", cwd=self.worktree)
        self.addCleanup(
            subprocess.run,
            ["git", "worktree", "remove", "--force", str(linked)],
            cwd=self.worktree,
            check=False,
            capture_output=True,
        )
        run("git", "push", "-u", "origin", branch, cwd=linked)
        _, _, slot, paths, _, _ = retirement.resource_plan_for(
            linked, self.env, cargo=self.cargo
        )
        private = self.cargo.private_worktree_root(slot, linked.resolve())
        slot.mkdir(parents=True, exist_ok=True)
        private.mkdir(parents=True, exist_ok=True)
        paths["target"].mkdir(exist_ok=True)
        paths["build"].mkdir(exist_ok=True)
        paths["temp"].mkdir(parents=True, exist_ok=True)
        (private / "allocation.json").write_text("{}\n", encoding="utf-8")
        record = {
            "slot_id": name,
            "path": str(linked),
            "branch": branch,
            "owner": None,
            "status": "retired",
            "last_used_at": "2026-11-09T00:00:00Z",
            "last_released_at": "2026-11-09T01:00:00Z",
        }

        def plan(created_at="2026-11-09T02:00:00Z"):
            return retirement.plan_retirement(
                linked,
                repo_root=self.worktree,
                slot_record=record,
                state_path=self.state_path,
                cleanup_reason="retired",
                env=self.env,
                created_at=created_at,
                cargo=self.cargo,
            )

        return linked, branch, paths, record, plan

    def test_plan_is_deterministic_and_read_only(self):
        first = self.plan()
        second = self.plan()
        self.assertEqual(first, second)
        self.assertEqual([], first["blockers"])
        self.assertEqual("MANAGED_ADMITTED", first["classification"])
        self.assertFalse(first["manifest"]["lease_state"]["missing_is_positive_proof"])
        self.assertEqual("NOT_PROVEN", first["manifest"]["live_consumer_state"])
        self.assertFalse(Path(first["manifest_path"]).exists())
        self.assertFalse(Path(first["manifest_path"]).parent.exists())

    def test_sibling_target_substitution_refuses_before_manifest_write(self):
        planned = self.plan()
        forged = copy.deepcopy(planned)
        sibling = self.root / "sibling-target"
        sibling.mkdir()
        forged["manifest"]["private_target_path"] = str(sibling)
        with self.assertRaisesRegex(retirement.RetirementRefused, "changed before mutation"):
            retirement.write_retirement_manifest(forged, replan=self.plan)
        self.assertFalse(Path(planned["manifest_path"]).exists())

    def test_write_readback_and_post_removal_identity_verification(self):
        planned = self.plan()
        receipt = retirement.write_retirement_manifest(planned, replan=self.plan)
        raw = Path(receipt["path"]).read_bytes()
        self.assertEqual(receipt["sha256"], hashlib.sha256(raw).hexdigest())
        parsed = json.loads(raw)
        self.assertEqual(retirement.SCHEMA, parsed["schema_version"])
        self.assertEqual("NOT_PROVEN", parsed["reclaim_eligibility"])
        shutil.rmtree(self.worktree)
        verification = retirement.verify_retained_assets(planned)
        self.assertEqual("RETAINED_IDENTITIES_MATCH", verification["verdict"])
        self.assertFalse(verification["reclaim_authorized"])
        self.assertTrue(self.paths["target"].is_dir())
        self.assertTrue(self.paths["build"].is_dir())

    def test_unrecorded_private_state_gets_exact_handoff_but_no_reclaim_claim(self):
        private = self.paths["target"].parent
        (private / "allocation.json").unlink()
        planned = self.plan()
        self.assertEqual("MANAGED_ADMITTED", planned["classification"])
        self.assertEqual([], planned["blockers"])
        self.assertEqual("unknown", planned["manifest"]["private_allocation_provenance"]["status"])
        self.assertEqual("NOT_PROVEN", planned["manifest"]["reclaim_eligibility"])
        receipt = retirement.write_retirement_manifest(planned, replan=self.plan)
        self.assertTrue(Path(receipt["path"]).is_file())

    def test_legacy_shared_state_is_classified_separately(self):
        private = self.paths["target"].parent
        shutil.rmtree(private)
        (self.devplane / "target").mkdir(parents=True)
        (self.devplane / "build").mkdir()
        planned = self.plan()
        self.assertEqual("LEGACY_SHARED", planned["classification"])
        self.assertIsNone(planned["manifest_path"])
        self.assertIn("legacy_shared_state_requires_separate_owner", planned["blockers"])

    def test_absent_external_state_is_unmanaged_and_manifestless(self):
        private = self.paths["target"].parent
        shutil.rmtree(private)
        planned = self.plan()
        self.assertEqual("UNMANAGED", planned["classification"])
        self.assertIsNone(planned["manifest_path"])
        self.assertEqual([], planned["blockers"])
        with self.assertRaisesRegex(retirement.RetirementRefused, "only MANAGED_ADMITTED"):
            retirement.write_retirement_manifest(planned, replan=self.plan)

    def test_real_git_worktree_removal_retains_external_assets(self):
        linked, _branch, paths, _record, plan = self.linked_plan("retire-real")
        planned = plan()
        receipt = retirement.write_retirement_manifest(planned, replan=plan)
        run("git", "worktree", "remove", str(linked), cwd=self.worktree)
        verification = retirement.verify_retained_assets(planned)
        self.assertEqual("RETAINED_IDENTITIES_MATCH", verification["verdict"])
        self.assertTrue(Path(receipt["path"]).is_file())
        self.assertTrue(paths["target"].is_dir())
        self.assertTrue(paths["build"].is_dir())

    def test_reused_worktree_path_rejects_old_plan(self):
        linked, _branch, _paths, _record, old_plan = self.linked_plan("reuse-path")
        planned = old_plan()
        run("git", "worktree", "remove", str(linked), cwd=self.worktree)
        new_branch = "fixture/reuse-path-new"
        run("git", "worktree", "add", "-b", new_branch, str(linked), "main", cwd=self.worktree)
        (linked / "generation.txt").write_text("new generation\n", encoding="utf-8")
        run("git", "add", "generation.txt", cwd=linked)
        run("git", "commit", "-m", "new generation", cwd=linked)
        run("git", "push", "-u", "origin", new_branch, cwd=linked)
        new_record = {**self.slot_record, "path": str(linked), "branch": new_branch}

        def replan():
            return retirement.plan_retirement(
                linked,
                repo_root=self.worktree,
                slot_record=new_record,
                state_path=self.state_path,
                cleanup_reason="retired",
                env=self.env,
                created_at="2026-11-09T02:00:00Z",
                cargo=self.cargo,
            )

        with self.assertRaisesRegex(retirement.RetirementRefused, "changed before mutation"):
            retirement.write_retirement_manifest(planned, replan=replan)

    def test_locked_worktree_is_review(self):
        linked, _branch, _paths, _record, plan = self.linked_plan("locked-source")
        run("git", "worktree", "lock", str(linked), cwd=self.worktree)
        self.addCleanup(
            subprocess.run,
            ["git", "worktree", "unlock", str(linked)],
            cwd=self.worktree,
            check=False,
            capture_output=True,
        )
        planned = plan()
        self.assertIn("source_worktree_locked", planned["blockers"])

    def test_untracked_source_is_review(self):
        (self.worktree / "untracked.txt").write_text("local\n", encoding="utf-8")
        planned = self.plan()
        self.assertIn("source_dirty", planned["blockers"])
        self.assertIn("source_untracked", planned["blockers"])

    def test_active_lease_blocks_handoff(self):
        lease = self.slot / "cargo-active"
        lease.mkdir()
        (lease / "owner-fixture").mkdir()
        planned = self.plan()
        self.assertIn("active_or_unsettled_cargo_lease", planned["blockers"])
        with self.assertRaisesRegex(retirement.RetirementRefused, "blocked"):
            retirement.write_retirement_manifest(planned, replan=self.plan)
        self.assertFalse(Path(planned["manifest_path"]).exists())

    def test_linked_target_is_review_and_preserved(self):
        shutil.rmtree(self.paths["target"])
        sibling = self.root / "sibling-target"
        sibling.mkdir()
        self.paths["target"].symlink_to(sibling, target_is_directory=True)
        planned = self.plan()
        self.assertIn("private_target_unexpected_symlink", planned["blockers"])
        self.assertTrue(self.paths["target"].is_symlink())

    def test_unpushed_commit_blocks_handoff(self):
        (self.worktree / "file.txt").write_text("fixture\nchanged\n", encoding="utf-8")
        run("git", "add", "file.txt", cwd=self.worktree)
        run("git", "commit", "-m", "local only", cwd=self.worktree)
        planned = self.plan()
        self.assertIn("source_unpushed", planned["blockers"])

    def test_manifest_destination_inside_retiring_worktree_refuses(self):
        with self.assertRaisesRegex(retirement.RetirementRefused, "inside the retiring worktree"):
            retirement.plan_retirement(
                self.worktree,
                repo_root=self.worktree,
                slot_record=self.slot_record,
                state_path=self.worktree / "state.json",
                cleanup_reason="retired",
                env=self.env,
                created_at="2026-11-09T02:00:00Z",
                cargo=self.cargo,
            )

    def test_devplane_inside_retiring_worktree_is_review(self):
        env = {**self.env, "DEVPLANE": str(self.worktree / ".devplane")}
        _, _, slot, paths, _, _ = retirement.resource_plan_for(self.worktree, env, cargo=self.cargo)
        private = self.cargo.private_worktree_root(slot, self.worktree.resolve())
        slot.mkdir(parents=True)
        private.mkdir(parents=True)
        paths["target"].mkdir()
        paths["build"].mkdir()
        paths["temp"].mkdir(parents=True, exist_ok=True)
        plan = retirement.plan_retirement(
            self.worktree,
            repo_root=self.worktree,
            slot_record=self.slot_record,
            state_path=self.state_path,
            cleanup_reason="retired",
            env=env,
            created_at="2026-11-09T02:00:00Z",
            cargo=self.cargo,
        )
        self.assertIn("private_target_inside_retiring_worktree", plan["blockers"])
        self.assertIn("private_build_inside_retiring_worktree", plan["blockers"])

    def test_fsync_failure_leaves_no_final_manifest(self):
        planned = self.plan()
        with patch.object(retirement.os, "fsync", side_effect=OSError("injected fsync failure")):
            with self.assertRaisesRegex(OSError, "injected"):
                retirement.write_retirement_manifest(planned, replan=self.plan)
        self.assertFalse(Path(planned["manifest_path"]).exists())
        self.assertEqual([], list(Path(planned["manifest_path"]).parent.glob("*.tmp")))

    def test_replace_failure_leaves_no_final_manifest(self):
        planned = self.plan()
        with patch.object(retirement.os, "replace", side_effect=OSError("injected replace failure")):
            with self.assertRaisesRegex(OSError, "injected"):
                retirement.write_retirement_manifest(planned, replan=self.plan)
        self.assertFalse(Path(planned["manifest_path"]).exists())
        self.assertEqual([], list(Path(planned["manifest_path"]).parent.glob("*.tmp")))

    def test_retained_identity_replacement_is_detected(self):
        planned = self.plan()
        shutil.rmtree(self.paths["target"])
        self.paths["target"].mkdir()
        with self.assertRaisesRegex(retirement.RetirementRefused, "identity changed"):
            retirement.verify_retained_assets(planned)


if __name__ == "__main__":
    unittest.main()
