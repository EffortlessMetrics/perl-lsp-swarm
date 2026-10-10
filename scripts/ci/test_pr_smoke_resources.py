#!/usr/bin/env python3
"""Cheap controls for resource selection; no Cargo or host budget overrides."""
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from contextlib import redirect_stderr

spec = importlib.util.spec_from_file_location("pr_smoke_resources", Path(__file__).with_name("pr_smoke_resources.py"))
subject = importlib.util.module_from_spec(spec)
spec.loader.exec_module(subject)


class SelectionTests(unittest.TestCase):
    def test_same_owner_paths_and_no_allocation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repo, plane = root / "repo", root / "plane"
            repo.mkdir()
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            env = {"DEVPLANE": str(plane), "CARGO_HOME": str(root / "cargo")}
            old = Path.cwd()
            try:
                os.chdir(repo)
                worktree, slot, paths = subject.resource_plan(env)
                result = subject.select(env)
                self.assertEqual(worktree, repo.resolve())
                self.assertEqual(result, f"CARGO_TARGET_DIR={paths['target']}\nCARGO_BUILD_BUILD_DIR={paths['build']}\n")
                # Same two paths are accepted by the actual parent owner.
                projected = dict(env, **dict(line.split("=", 1) for line in result.splitlines()))
                self.assertEqual(subject.resource_plan(projected), (worktree, slot, paths))
                self.assertFalse(plane.exists())
                self.assertNotIn("CARGO_ADMITTED_RESOURCES", result)
            finally:
                os.chdir(old)

    def test_linked_or_ambient_stale_roots_refuse_without_allocation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repo = root / "repo"
            repo.mkdir()
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            old = Path.cwd()
            try:
                os.chdir(repo)
                for name in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "GIT_WORK_TREE"):
                    with self.subTest(name=name), self.assertRaises(subject.Denied):
                        subject.select({"DEVPLANE": str(root / "plane"), name: str(root / "stale")})
                link = root / "linked"
                link.symlink_to(repo, target_is_directory=True)
                with self.assertRaises(subject.Denied):
                    subject.select({"DEVPLANE": str(link / "plane")})
                self.assertFalse((root / "plane").exists())
            finally:
                os.chdir(old)

    def test_refuses_line_injection_before_writing(self):
        for value in ("/plane\nFAKE=value/target", "/plane\rFAKE=value/target"):
            paths = {"target": Path(value), "build": Path("/plane/build")}
            with tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / "env"
                output.write_text("existing=value\n", encoding="utf-8")
                with patch.object(subject, "resource_plan", return_value=(Path("/repo"), Path("/plane"), paths)), patch.dict(os.environ, {"GITHUB_ENV": str(output)}, clear=True), redirect_stderr(io.StringIO()):
                    self.assertEqual(subject.main(), 75)
                self.assertEqual(output.read_text(), "existing=value\n")

    def test_both_values_appended_without_descriptor_or_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "env"
            output.write_text("existing=value\n", encoding="utf-8")
            paths = {"target": Path("/private/target"), "build": Path("/private/build")}
            with patch.object(subject, "resource_plan", return_value=(Path("/repo"), Path("/private"), paths)), patch.dict(os.environ, {"GITHUB_ENV": str(output)}, clear=True), patch.object(subprocess, "call", side_effect=AssertionError("must not launch Cargo")):
                self.assertEqual(subject.main(), 0)
            self.assertEqual(output.read_text(), "existing=value\nCARGO_TARGET_DIR=/private/target\nCARGO_BUILD_BUILD_DIR=/private/build\n")

    def test_failed_git_identity_cannot_publish_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "env"
            with patch.object(subject, "resource_plan", side_effect=subprocess.CalledProcessError(128, ["git", "rev-parse"])), patch.dict(os.environ, {"GITHUB_ENV": str(output)}, clear=True), redirect_stderr(io.StringIO()):
                self.assertEqual(subject.main(), 75)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
