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
EXPECTED_PR_BASE_BINDING = (
    "PR_BASE_SHA: ${{ github.event_name == 'pull_request' "
    "&& github.event.pull_request.base.sha || '' }}"
)
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


def python_executable() -> str:
    """Use the interpreter name exposed by the host's shell environment."""
    return "python" if os.name == "nt" else "python3"


def executable_workflow_script() -> str:
    """Adapt only the host interpreter name while preserving the workflow body."""
    script = candidate_self_test_script()
    return script.replace("python3 -m unittest", f"{python_executable()} -m unittest")


def assert_pr_base_binding(step: str) -> None:
    """Require the immutable pull-request base expression, exactly once."""
    bindings = [
        line.strip()
        for line in step.splitlines()
        if line.strip().startswith("PR_BASE_SHA:")
    ]
    if bindings != [EXPECTED_PR_BASE_BINDING]:
        raise AssertionError(
            "the meta self-test step must bind PR_BASE_SHA exactly to "
            f"{EXPECTED_PR_BASE_BINDING!r}; found {bindings!r}"
        )


def initialize_base(root: Path, files: dict[Path, str] | None = None) -> str:
    """Create the immutable PR-base fixture and return its commit id."""
    subprocess.run(["git", "init", "-q"], cwd=root, check=True)
    subprocess.run(
        ["git", "config", "user.name", "CI fixture"], cwd=root, check=True
    )
    subprocess.run(
        ["git", "config", "user.email", "ci-fixture@example.invalid"],
        cwd=root,
        check=True,
    )
    for relative, source in (files or {}).items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source, encoding="utf-8")
    subprocess.run(["git", "add", "--all"], cwd=root, check=True)
    subprocess.run(
        ["git", "commit", "-q", "--allow-empty", "-m", "base fixture"],
        cwd=root,
        check=True,
    )
    return subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()


def commit_fixture(root: Path, message: str) -> str:
    """Commit the current fixture tree and return its commit id."""
    subprocess.run(["git", "add", "--all"], cwd=root, check=True)
    subprocess.run(["git", "commit", "-q", "-m", message], cwd=root, check=True)
    return subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()


def initialize_criss_cross(root: Path) -> str:
    """Create two branch tips with two equally good merge bases."""
    base_sha = initialize_base(root)
    subprocess.run(
        ["git", "switch", "-c", "candidate", base_sha],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    (root / "candidate.txt").write_text("candidate\n", encoding="utf-8")
    candidate_sha = commit_fixture(root, "candidate side")
    subprocess.run(
        ["git", "switch", "-c", "main-side", base_sha],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    (root / "main.txt").write_text("main\n", encoding="utf-8")
    main_sha = commit_fixture(root, "main side")
    subprocess.run(
        ["git", "merge", "--no-ff", "--no-edit", candidate_sha],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    main_tip = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    subprocess.run(
        ["git", "switch", "candidate"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    subprocess.run(
        ["git", "merge", "--no-ff", "--no-edit", main_sha],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    return main_tip


class CandidateMetaSelfTestContract(unittest.TestCase):
    maxDiff = None

    def run_workflow_script(
        self,
        files: dict[Path, str] | None = None,
        directories: tuple[Path, ...] = (),
        base_files: dict[Path, str] | None = None,
        include_pr_base: bool = True,
        pr_base_override: str | None = None,
        diverged_main_files: dict[Path, str] | None = None,
    ) -> tuple[subprocess.CompletedProcess[str], str]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            base_sha = initialize_base(root, base_files)
            pr_base_sha = base_sha
            if diverged_main_files is not None:
                for relative, source in diverged_main_files.items():
                    path = root / relative
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text(source, encoding="utf-8")
                pr_base_sha = commit_fixture(root, "main-only fixture")
                subprocess.run(
                    ["git", "switch", "-c", "candidate", base_sha],
                    cwd=root,
                    check=True,
                    capture_output=True,
                    text=True,
                )

            for relative in SELF_TESTS:
                path = root / relative
                if path.is_symlink() or path.is_file():
                    path.unlink()
                elif path.is_dir():
                    shutil.rmtree(path)
            for relative in directories:
                (root / relative).mkdir(parents=True, exist_ok=True)
            for relative, source in (files or {}).items():
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source, encoding="utf-8")

            summary = root / "summary.md"
            environment = os.environ.copy()
            environment["GITHUB_STEP_SUMMARY"] = str(summary)
            environment["PR_BASE_SHA"] = (
                pr_base_override
                if pr_base_override is not None
                else pr_base_sha if include_pr_base else ""
            )
            result = subprocess.run(
                [bash_executable(), "-c", executable_workflow_script()],
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

    def test_candidate_deletion_of_base_test_stays_red(self) -> None:
        scope_test = Path("scripts/ci/test_scope_cache_key.py")
        candidate_files = {path: passing_test() for path in SELF_TESTS}
        candidate_files.pop(scope_test)

        result, summary = self.run_workflow_script(
            candidate_files,
            base_files={scope_test: passing_test()},
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("exists in historical PR base", result.stdout)
        self.assertNotIn(f"`{scope_test.as_posix()}`", summary)

    def test_main_added_path_is_not_candidate_deletion_on_diverged_history(self) -> None:
        scope_test = Path("scripts/ci/test_scope_cache_key.py")
        candidate_files = {path: passing_test() for path in SELF_TESTS}
        candidate_files.pop(scope_test)

        result, summary = self.run_workflow_script(
            candidate_files,
            diverged_main_files={scope_test: passing_test()},
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(
            f"STALE_HEAD_SCOPED_NOOP: candidate path {scope_test.as_posix()} is absent",
            result.stdout,
        )
        self.assertIn(f"`{scope_test.as_posix()}`", summary)

    def test_ambiguous_criss_cross_history_stays_red(self) -> None:
        scope_test = Path("scripts/ci/test_scope_cache_key.py")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pr_base_sha = initialize_criss_cross(root)
            for relative in SELF_TESTS:
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
            environment = os.environ.copy()
            environment["GITHUB_STEP_SUMMARY"] = str(root / "summary.md")
            environment["PR_BASE_SHA"] = pr_base_sha
            result = subprocess.run(
                [bash_executable(), "-c", executable_workflow_script()],
                cwd=root,
                env=environment,
                capture_output=True,
                text=True,
                check=False,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("ambiguous or unavailable", result.stdout)
        self.assertNotIn(f"STALE_HEAD_SCOPED_NOOP: candidate path {scope_test.as_posix()}", result.stdout)

    def test_absent_test_without_pr_base_stays_red(self) -> None:
        result, summary = self.run_workflow_script(include_pr_base=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("without an immutable PR base SHA", result.stdout)
        self.assertEqual(summary, "")

    def test_absent_test_with_unavailable_pr_base_stays_red(self) -> None:
        result, summary = self.run_workflow_script(
            pr_base_override="a" * 40,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("is unavailable", result.stdout)
        self.assertEqual(summary, "")

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
            base_sha = initialize_base(root)
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
            environment["PR_BASE_SHA"] = base_sha
            result = subprocess.run(
                [bash_executable(), "-c", executable_workflow_script()],
                cwd=root,
                env=environment,
                capture_output=True,
                text=True,
                check=False,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not a regular non-symlink file", result.stdout)

    def test_symlinked_ancestor_candidate_test_stays_red(self) -> None:
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as outside:
            root = Path(directory)
            outside_root = Path(outside)
            initialize_base(root)
            outside_ci = outside_root / "ci"
            outside_ci.mkdir()
            for relative in SELF_TESTS:
                target = outside_ci / relative.name
                target.write_text(passing_test(), encoding="utf-8")

            scripts = root / "scripts"
            scripts.mkdir()
            try:
                (scripts / "ci").symlink_to(outside_ci, target_is_directory=True)
            except OSError as error:
                self.skipTest(f"directory symlinks unavailable: {error}")

            environment = os.environ.copy()
            environment["GITHUB_STEP_SUMMARY"] = str(root / "summary.md")
            environment["PR_BASE_SHA"] = "a" * 40
            result = subprocess.run(
                [bash_executable(), "-c", executable_workflow_script()],
                cwd=root,
                env=environment,
                capture_output=True,
                text=True,
                check=False,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("escapes checkout", result.stdout)

    def test_workflow_uses_only_candidate_paths(self) -> None:
        script = candidate_self_test_script()
        workflow = WORKFLOW.read_text(encoding="utf-8")
        step = workflow.split(f"      - name: {STEP_NAME}", 1)[1].split(
            "\n      - name:", 1
        )[0]

        for path in SELF_TESTS:
            self.assertIn(path.as_posix(), script)
        self.assertIn("        if: matrix.name == 'meta'", step)
        self.assertIn("        shell: bash", step)
        assert_pr_base_binding(step)
        self.assertIn('git cat-file -e "${PR_BASE_SHA}^{commit}"', script)
        self.assertIn('mapfile -t historical_base_shas < <(git merge-base --all HEAD "$PR_BASE_SHA")', script)
        self.assertIn('[[ "${#historical_base_shas[@]}" -ne 1 ]]', script)
        self.assertIn('historical_base_sha="${historical_base_shas[0]}"', script)
        self.assertIn('git cat-file -e "${historical_base_sha}:${self_test}"', script)
        self.assertIn('checkout_root="$(realpath -e -- .)"', script)
        self.assertIn('resolved_self_test="$(realpath -e -- "$self_test")"', script)
        for forbidden in (
            "git show",
            "git checkout",
            "git fetch",
            "origin/main",
            "curl ",
            "wget ",
        ):
            self.assertNotIn(forbidden, script)

    def test_workflow_rejects_head_or_wrong_context_binding(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        step = workflow.split(f"      - name: {STEP_NAME}", 1)[1].split(
            "\n      - name:", 1
        )[0]
        for wrong_expression in (
            "github.event.pull_request.head.sha",
            "github.sha",
            "github.event.head_commit.id",
        ):
            with self.subTest(wrong_expression=wrong_expression):
                wrong_step = step.replace(
                    "github.event.pull_request.base.sha", wrong_expression
                )
                with self.assertRaises(AssertionError):
                    assert_pr_base_binding(wrong_step)


if __name__ == "__main__":
    unittest.main()
