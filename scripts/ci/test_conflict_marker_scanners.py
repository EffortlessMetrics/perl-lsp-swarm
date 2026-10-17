#!/usr/bin/env python3
"""Run both real scan bodies against tracked, Git-generated conflict fixtures."""

from __future__ import annotations

import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]
CONTRACT_COMMAND = '"$RUNNER_TEMP/required-ci-regression/bin/python" scripts/ci/test_conflict_marker_scanners.py'


def scan_bodies() -> dict[str, str]:
    workflow = yaml.safe_load((ROOT / ".github/workflows/ci.yml").read_text())
    step = next(step for step in workflow["jobs"]["conflict-markers"]["steps"]
                if step.get("name") == "Check for conflict markers")
    source = (ROOT / "justfile").read_text()
    match = re.search(r"^check-conflict-markers:\n((?:    .*\n|\n)+)", source, re.M)
    if not match:
        raise AssertionError("missing check-conflict-markers recipe")
    recipe = "\n".join(line[4:] for line in match[1].splitlines() if line.startswith("    "))
    return {"workflow": step["run"], "justfile": recipe}


class ConflictMarkerScannerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        for selector in ("GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR", "GIT_INDEX_FILE"):
            self.env.pop(selector, None)
        self.git("init", "-q")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, env=self.env,
                              capture_output=True, timeout=10, check=True)

    def tracked(self, content: bytes, filename="tracked source.txt"):
        path = self.root / filename
        path.write_bytes(content)
        self.git("add", "--", filename)
        return path

    def assert_scan(self, expected):
        for name, body in scan_bodies().items():
            with self.subTest(scanner=name):
                result = subprocess.run(["bash", "--noprofile", "--norc", "-euo", "pipefail",
                                         "-c", body], cwd=self.root, env=self.env,
                                        capture_output=True, text=True, timeout=10)
                self.assertEqual(expected, result.returncode, result.stdout + result.stderr)

    def git_conflict(self, size, style):
        for name, content in (("ours", "ours\n"), ("base", "base\n"), ("theirs", "theirs\n")):
            (self.root / name).write_text(content)
        style_args = [] if style == "merge" else [f"--{style}"]
        result = subprocess.run(["git", "merge-file", "-p", f"--marker-size={size}",
                                 *style_args, "ours", "base", "theirs"], cwd=self.root,
                                env=self.env, capture_output=True, timeout=10)
        self.assertEqual(1, result.returncode, result.stderr)
        self.assertIn(b"<" * size + b" ours", result.stdout)
        return result.stdout

    def test_real_default_and_longer_git_conflicts_are_rejected(self):
        for size in (7, 8, 32):
            for style in ("merge", "diff3", "zdiff3"):
                with self.subTest(size=size, style=style):
                    self.tracked(self.git_conflict(size, style))
                    self.assert_scan(1)

    def test_partial_marker_lines_are_rejected(self):
        for marker in (b"<", b">", b"|"):
            for size in (7, 8, 32):
                with self.subTest(marker=marker, size=size):
                    self.tracked(marker * size + b" branch\n")
                    self.assert_scan(1)
        self.tracked(b"=" * 7 + b"\n")
        self.assert_scan(1)

    def test_prose_headings_and_indented_examples_stay_clean(self):
        self.tracked(b"========\n================================\n"
                     b"prose <<<<<<<< branch\n    <<<<<<<< example\n"
                     b">>>>>>>>no-space\n||||||||no-space\n<<<<<< short\n"
                     b"inline `<<<<<<< example`\n")
        self.assert_scan(0)

    def test_untracked_conflict_and_binary_content_are_ignored(self):
        (self.root / "untracked.txt").write_bytes(self.git_conflict(8, "diff3"))
        self.tracked(b"\0<<<<<<< binary\n")
        self.assert_scan(0)

    def test_empty_repository_stays_clean(self):
        self.assert_scan(0)

    def test_scanner_instrument_failure_does_not_report_clean(self):
        # A missing repository is an instrument failure, not "no matches".
        outside = self.root / "outside"
        outside.mkdir()
        for name, body in scan_bodies().items():
            with self.subTest(scanner=name):
                result = subprocess.run(["bash", "--noprofile", "--norc", "-euo", "pipefail",
                                         "-c", body], cwd=outside, env=dict(self.env,
                                         GIT_CEILING_DIRECTORIES=str(self.root)),
                                        capture_output=True, text=True, timeout=10)
                self.assertNotEqual(0, result.returncode, result.stdout + result.stderr)
                self.assertNotIn("No conflict markers found", result.stdout)

    def test_filename_with_spaces_or_newline_does_not_hide_conflict(self):
        self.tracked(self.git_conflict(8, "merge"), "space and\nnewline.txt")
        self.assert_scan(1)

    def test_contract_runs_on_existing_required_compile_surface(self):
        workflow = yaml.safe_load((ROOT / ".github/workflows/ci.yml").read_text())
        self.assertTrue(any(step.get("run") == CONTRACT_COMMAND
                            and "if" not in step and "continue-on-error" not in step
                            for step in workflow["jobs"]["check-all-targets"]["steps"]))


if __name__ == "__main__":
    unittest.main()
