#!/usr/bin/env python3
"""Contract tests for stale-head-compatible candidate meta self-tests."""

from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
STEP_NAME = "Verify candidate meta self-tests"
SELF_TESTS = (
    Path("scripts/ci/test_run_gate_shard.py"),
    Path("scripts/ci/test_scope_cache_key.py"),
    Path("scripts/ci/test_candidate_meta_self_tests.py"),
)


def candidate_self_test_script() -> str:
    """Extract the exact shell program executed by the candidate meta step."""
    lines = WORKFLOW.read_text(encoding="utf-8").splitlines()
    name_line = f"      - name: {STEP_NAME}"
    try:
        start = lines.index(name_line)
    except ValueError as error:
        raise RuntimeError(f"workflow step is absent: {STEP_NAME}") from error

    run_index = None
    for index in range(start + 1, len(lines)):
        if lines[index].startswith("      - name:"):
            break
        if lines[index] == "        run: |":
            run_index = index
            break
    if run_index is None:
        raise RuntimeError(f"workflow step has no literal run block: {STEP_NAME}")

    body: list[str] = []
    for line in lines[run_index + 1 :]:
        if line and not line.startswith("          "):
            break
        body.append(line[10:] if line else "")
    if not body:
        raise RuntimeError(f"workflow step has an empty run block: {STEP_NAME}")
    return "\n".join(body) + "\n"


def passing_test() -> str:
    return textwrap.dedent(
        """\
        import unittest

        class PassingFixture(unittest.TestCase):
            def test_passes(self):
                return None
        """
    )


def bash_executable() -> str:
    """Prefer Git Bash on Windows; the legacy WSL launcher can retain cwd locks."""
    if os.name == "nt":
        program_files = os.environ.get("ProgramFiles")
        if program_files:
            git_bash = Path(program_files) / "Git" / "bin" / "bash.exe"
            if git_bash.is_file():
                return str(git_bash)
    executable = shutil.which("bash")
    if executable is None:
        raise RuntimeError("bash is required to exercise the workflow run block")
    return executable


class CandidateMetaSelfTestContract(unittest.TestCase):
    maxDiff = None

    def run_workflow_script(
        self,
        files: dict[Path, str] | None = None,
        directories: tuple[Path, ...] = (),
    ) -> tuple[subprocess.CompletedProcess[str], str]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative in directories:
                (root / relative).mkdir(parents=True, exist_ok=True)
            for relative, source in (files or {}).items():
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source, encoding="utf-8")

            summary = root / "summary.md"
            environment = os.environ.copy()
            environment["GITHUB_STEP_SUMMARY"] = str(summary)
            result = subprocess.run(
                [bash_executable(), "-c", candidate_self_test_script()],
                cwd=root,
                env=environment,
                capture_output=True,
                text=True,
                check=False,
            )
            summary_text = (
                summary.read_text(encoding="utf-8") if summary.exists() else ""
            )
            return result, summary_text

    def test_absent_candidate_tests_are_typed_scoped_noops(self) -> None:
        result, summary = self.run_workflow_script()

        self.assertEqual(result.returncode, 0, result.stderr)
        for path in SELF_TESTS:
            self.assertIn(
                f"STALE_HEAD_SCOPED_NOOP: candidate path {path.as_posix()} is absent",
                result.stdout,
            )
            self.assertIn(f"`{path.as_posix()}`", summary)
        self.assertEqual(summary.count("`STALE_HEAD_SCOPED_NOOP`"), len(SELF_TESTS))

    def test_present_candidate_tests_all_run_and_pass(self) -> None:
        result, summary = self.run_workflow_script(
            {path: passing_test() for path in SELF_TESTS}
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(summary, "")
        self.assertEqual(result.stderr.count("Ran 1 test"), len(SELF_TESTS))

    def test_present_failing_candidate_test_stays_red(self) -> None:
        failing = textwrap.dedent(
            """\
            import unittest

            class FailingFixture(unittest.TestCase):
                def test_fails(self):
                    self.fail("controlled candidate failure")
            """
        )
        files = {path: passing_test() for path in SELF_TESTS}
        files[Path("scripts/ci/test_scope_cache_key.py")] = failing

        result, _ = self.run_workflow_script(files)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("controlled candidate failure", result.stderr)

    def test_present_unloadable_candidate_test_stays_red(self) -> None:
        files = {path: passing_test() for path in SELF_TESTS}
        files[Path("scripts/ci/test_scope_cache_key.py")] = (
            "import definitely_absent_candidate_helper\n"
        )

        result, _ = self.run_workflow_script(files)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("definitely_absent_candidate_helper", result.stderr)

    def test_present_non_file_candidate_test_stays_red(self) -> None:
        files = {path: passing_test() for path in SELF_TESTS}
        scope_test = Path("scripts/ci/test_scope_cache_key.py")
        files.pop(scope_test)

        result, _ = self.run_workflow_script(files, directories=(scope_test,))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not a regular non-symlink file", result.stdout)

    def test_present_broken_symlink_candidate_test_stays_red(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative in SELF_TESTS:
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(passing_test(), encoding="utf-8")
            scope_test = root / "scripts/ci/test_scope_cache_key.py"
            scope_test.unlink()
            try:
                scope_test.symlink_to("missing-helper-test.py")
            except OSError as error:
                self.skipTest(f"symlink creation unavailable: {error}")

            environment = os.environ.copy()
            environment["GITHUB_STEP_SUMMARY"] = str(root / "summary.md")
            result = subprocess.run(
                [bash_executable(), "-c", candidate_self_test_script()],
                cwd=root,
                env=environment,
                capture_output=True,
                text=True,
                check=False,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not a regular non-symlink file", result.stdout)

    def test_workflow_uses_only_candidate_paths(self) -> None:
        script = candidate_self_test_script()

        for path in SELF_TESTS:
            self.assertIn(path.as_posix(), script)
        for forbidden in ("git show", "git checkout", "git fetch", "curl ", "wget "):
            self.assertNotIn(forbidden, script)


if __name__ == "__main__":
    unittest.main()
