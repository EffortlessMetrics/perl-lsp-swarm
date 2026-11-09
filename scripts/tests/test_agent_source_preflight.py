#!/usr/bin/env python3
"""Compiler-free source admission and retained local-Git preflight controls."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


PREFLIGHT = Path(__file__).resolve().parents[1] / "agent-preflight.sh"


class SourcePreflightTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="source-preflight-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "candidate with spaces"
        self.repo.mkdir()
        self.env = {
            key: value for key, value in os.environ.items()
            if not key.startswith("GIT_") and key != "CARGO_TARGET_DIR"
        }
        self.env.update({"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull})
        self.git("init", "-q", "-b", "candidate")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        (self.repo / "README").write_text("fixture\n", encoding="utf-8")
        (self.repo / "hooks").mkdir()
        (self.repo / "hooks/pre-push").write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        self.git("add", "README", "hooks/pre-push")
        self.git("commit", "-qm", "fixture")

    def git(self, *args: str, cwd: Path | None = None, check: bool = True) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["git", *args], cwd=cwd or self.repo, env=self.env,
            text=True, capture_output=True, timeout=5, check=check,
        )

    def linked(self) -> Path:
        path = self.root / "linked candidate"
        self.git("worktree", "add", "-qb", "linked", str(path))
        return path

    def invoke(self, *args: str, cwd: Path | None = None, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(PREFLIGHT), *args], cwd=cwd or self.repo,
            env=self.env | (env or {}), text=True, capture_output=True, timeout=5,
        )

    def assert_code(self, expected: int, result: subprocess.CompletedProcess[str]) -> None:
        self.assertEqual(expected, result.returncode, result.stdout + result.stderr)

    def install_fixture_hooks(self, cwd: Path) -> Path:
        hooks = Path(self.git("rev-parse", "--path-format=absolute", "--git-path", "hooks", cwd=cwd).stdout.strip())
        hooks.mkdir(exist_ok=True)
        shutil.copyfile(cwd / "hooks/pre-push", hooks / "pre-push")
        (hooks / "pre-push").chmod(0o755)
        (hooks / "pre-commit").write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        (hooks / "pre-commit").chmod(0o755)
        return hooks

    def test_source_standalone_needs_no_hooks_or_cargo(self) -> None:
        tools = self.root / "tools"
        tools.mkdir()
        called = self.root / "unexpected-build-tool"
        for name in ("cargo", "rustc", "just"):
            tool = tools / name
            tool.write_text('#!/bin/sh\n: > "$BUILD_TOOL_CALLED"\nexit 97\n', encoding="utf-8")
            tool.chmod(0o755)
        before = self.git("status", "--porcelain").stdout
        result = self.invoke("--source", env={
            "PATH": str(tools) + os.pathsep + self.env["PATH"],
            "BUILD_TOOL_CALLED": str(called),
        })
        self.assert_code(0, result)
        self.assertIn("Source preflight passed", result.stdout)
        self.assertIn("not assessed", result.stdout)
        self.assertNotIn("install-githooks", result.stdout)
        self.assertFalse((self.repo / ".githooks").exists())
        self.assertFalse((self.repo / "target").exists())
        self.assertFalse(called.exists(), "source preflight invoked a build tool")
        self.assertEqual(before, self.git("status", "--porcelain").stdout)

    def test_source_linked_checkout_needs_no_hooks(self) -> None:
        self.assert_code(0, self.invoke("--source", cwd=self.linked()))

    def test_source_standalone_from_subdirectory(self) -> None:
        nested = self.repo / "nested"
        nested.mkdir()
        self.assert_code(0, self.invoke("--source", cwd=nested))

    def test_source_standalone_separate_git_directory(self) -> None:
        separate = self.root / "git metadata"
        self.git("init", "-q", "--separate-git-dir", str(separate))
        self.assert_code(0, self.invoke("--source"))

    def test_source_does_not_treat_cargo_environment_as_edit_authority(self) -> None:
        self.assert_code(0, self.invoke("--source", env={"CARGO_TARGET_DIR": "/not-an-admitted-build"}))

    def test_source_does_not_claim_stale_hooks_are_current(self) -> None:
        linked = self.linked()
        hooks = self.install_fixture_hooks(linked)
        (hooks / "pre-push").write_text("#!/bin/sh\nexit 99\n", encoding="utf-8")
        result = self.invoke("--source", cwd=linked)
        self.assert_code(0, result)
        self.assertNotIn("hook is current", result.stdout)

    def test_source_rejects_main_and_master(self) -> None:
        for branch in ("main", "master"):
            with self.subTest(branch=branch):
                self.git("branch", "-m", branch)
                result = self.invoke("--source")
                self.assert_code(1, result)
                self.assertIn("candidate branch in the owned checkout", result.stdout)
                self.assertNotIn("isolation: worktree", result.stdout)

    def test_source_rejects_detached_head(self) -> None:
        self.git("checkout", "-q", "--detach")
        self.assert_code(1, self.invoke("--source"))

    def test_source_rejects_outside_git(self) -> None:
        self.assert_code(2, self.invoke("--source", cwd=self.root))

    def test_source_rejects_bare_repository(self) -> None:
        bare = self.root / "bare.git"
        self.git("init", "--bare", "-q", str(bare))
        self.assert_code(2, self.invoke("--source", cwd=bare))

    def test_source_rejects_repository_environment_redirection(self) -> None:
        for key in ("GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"):
            with self.subTest(key=key):
                self.assert_code(2, self.invoke("--source", env={key: str(self.repo / ".git")}))

    def test_source_rejects_shared_primary_checkout(self) -> None:
        self.linked()
        self.assert_code(2, self.invoke("--source"))

    def test_source_rejects_conflict_markers(self) -> None:
        (self.repo / "conflicted.txt").write_text("<<<<<<< HEAD\nleft\n=======\nright\n>>>>>>> other\n", encoding="utf-8")
        self.assert_code(3, self.invoke("--source"))

    def test_source_nested_invocation_checks_conflicts_at_checkout_root(self) -> None:
        nested = self.repo / "nested"
        nested.mkdir()
        (self.repo / "conflicted.txt").write_text("<<<<<<< HEAD\n", encoding="utf-8")
        self.assert_code(3, self.invoke("--source", cwd=nested))

    def create_unmerged_index_without_markers(self) -> None:
        self.git("checkout", "-qb", "other")
        (self.repo / "README").write_text("other\n", encoding="utf-8")
        self.git("commit", "-qam", "other")
        self.git("checkout", "-q", "candidate")
        (self.repo / "README").write_text("candidate\n", encoding="utf-8")
        self.git("commit", "-qam", "candidate")
        self.assertEqual(1, self.git("merge", "other", check=False).returncode)
        (self.repo / "README").write_text("resolved but not staged\n", encoding="utf-8")

    def test_source_rejects_unmerged_index_without_markers(self) -> None:
        self.create_unmerged_index_without_markers()
        self.assert_code(3, self.invoke("--source"))

    def test_source_rejects_alternate_index_that_hides_conflicts(self) -> None:
        self.create_unmerged_index_without_markers()
        self.assert_code(3, self.invoke("--source"))
        alternate = {"GIT_INDEX_FILE": str(self.root / "clean-index")}
        subprocess.run(
            ["git", "read-tree", "HEAD"], cwd=self.repo, env=self.env | alternate,
            text=True, capture_output=True, timeout=5, check=True,
        )
        self.assert_code(2, self.invoke("--source", env=alternate))
        self.assertEqual("README\n", self.git("diff", "--name-only", "--diff-filter=U").stdout)

    def test_source_rejects_shared_stash(self) -> None:
        linked = self.linked()
        (linked / "README").write_text("unpreserved\n", encoding="utf-8")
        self.git("stash", "push", "-qm", "fixture", cwd=linked)
        self.assert_code(6, self.invoke("--source", cwd=linked))

    def test_default_still_requires_linked_worktree(self) -> None:
        self.install_fixture_hooks(self.repo)
        self.assert_code(2, self.invoke())

    def test_default_still_refuses_missing_hooks(self) -> None:
        self.assert_code(7, self.invoke(cwd=self.linked()))

    def test_default_still_refuses_stale_hooks(self) -> None:
        linked = self.linked()
        hooks = self.install_fixture_hooks(linked)
        (hooks / "pre-push").write_text("#!/bin/sh\nexit 99\n", encoding="utf-8")
        self.assert_code(7, self.invoke(cwd=linked))

    def test_default_still_refuses_missing_precommit(self) -> None:
        linked = self.linked()
        hooks = self.install_fixture_hooks(linked)
        (hooks / "pre-commit").unlink()
        self.assert_code(7, self.invoke(cwd=linked))

    def test_default_still_checks_cargo_target(self) -> None:
        linked = self.linked()
        self.install_fixture_hooks(linked)
        self.assert_code(5, self.invoke(cwd=linked, env={"CARGO_TARGET_DIR": "/wrong-target"}))

    def test_default_valid_hook_complement(self) -> None:
        linked = self.linked()
        self.install_fixture_hooks(linked)
        self.assert_code(0, self.invoke(cwd=linked))

    def test_unknown_mode_is_not_silently_accepted(self) -> None:
        linked = self.linked()
        self.install_fixture_hooks(linked)
        self.assert_code(64, self.invoke("--soruce", cwd=linked))

    def test_help_has_no_repository_or_setup_prerequisite(self) -> None:
        for flag in ("--help", "-h"):
            with self.subTest(flag=flag):
                result = self.invoke(flag, cwd=self.root)
                self.assert_code(0, result)
                self.assertIn("--source", result.stdout)

    def test_extra_arguments_are_not_silently_accepted(self) -> None:
        self.assert_code(64, self.invoke("--source", "--source"))


if __name__ == "__main__":
    unittest.main()
