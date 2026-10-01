#!/usr/bin/env python3
"""Discriminating contracts for the script-mediated governed Rust Small route.

All routes use one pinned reusable workflow and one owned proof script. Mutation
controls retain the formatter/canonical-command parity guarantees of #8407/#8408.
"""
from __future__ import annotations

import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = ROOT / ".github/workflows/em-ci-routed-rust.yml"
PROOF_PATH = ROOT / ".ci/rust-standard-proof.sh"
CANONICAL_INVOCATION = "cargo run -p xtask --locked -- rust-small-proof"
CONTRACT_TEST_FILE = "scripts/ci/test_rust_small_route_contract.py"
LEGACY_INLINE_FRAGMENTS = (
    'awk "/: test', "grep -c -F", "-p perl-parser --test semantic_smoke_tests",
    "-p perl-parser --test parser_accuracy_e2e", "-p perl-lsp-rs --test lsp_smoke",
    "references_tier_scorecard_tests", "cargo fetch --locked",
    "cargo check --workspace --locked", "INSTA_UPDATE=no",
)
ALLOWED_CARGO_COMMANDS = ("cargo fmt --all -- --check", CANONICAL_INVOCATION)


def load_workflow_text() -> str:
    return WORKFLOW_PATH.read_text(encoding="utf-8")


def job_bodies(workflow_text: str) -> dict[str, str]:
    """Return indent-2 GitHub Actions job bodies keyed by job id."""
    bodies: dict[str, list[str]] = {}
    current: str | None = None
    in_jobs = False
    for line in workflow_text.splitlines():
        if line == "jobs:":
            in_jobs = True
            current = None
            continue
        if in_jobs and line and not line.startswith((" ", "\t")):
            in_jobs = False
            current = None
            continue
        if not in_jobs:
            continue
        if (
            line.startswith("  ")
            and not line.startswith("   ")
            and line.rstrip().endswith(":")
            and not line.lstrip().startswith("-")
        ):
            current = line.strip()[:-1]
            bodies[current] = [line]
        elif current is not None:
            bodies[current].append(line)
    return {job_id: "\n".join(lines) for job_id, lines in bodies.items()}


def active_code_lines(text: str) -> list[str]:
    lines: list[str] = []
    for raw in text.splitlines():
        stripped = raw.strip()
        if not stripped or stripped.startswith("#"):
            continue
        lines.append(raw)
    return lines


def canonical_invocation_count(job_body: str) -> int:
    return sum(
        1
        for line in active_code_lines(job_body)
        if line.strip().strip("'\"") == CANONICAL_INVOCATION
    )


def validate_rust_small_route_contract(workflow_text: str, proof_text: str | None = None) -> None:
    """Validate execution reachability as well as the canonical proof body."""
    proof_text = PROOF_PATH.read_text(encoding="utf-8") if proof_text is None else proof_text
    workflow = yaml.safe_load(workflow_text)
    jobs = workflow.get("jobs", {})
    call = jobs.get("rust-small-proof")
    if not isinstance(call, dict):
        raise AssertionError("governed Rust Small proof job missing")
    if not re.fullmatch(r"EffortlessMetrics/em-ci-workflows/\.github/workflows/rust\.yml@[0-9a-f]{40}", call.get("uses", "")):
        raise AssertionError("governed workflow must use a full-SHA pin")
    if set(call) != {"name", "if", "uses", "with"}:
        raise AssertionError("thin caller must not own runners, permissions, secrets or implementation")
    inputs = call.get("with", {})
    if set(inputs) != {"profile", "script", "fetch_depth", "force_hosted"} or inputs.get("profile") != "standard" or inputs.get("script") != ".ci/rust-standard-proof.sh" or inputs.get("fetch_depth") != 1:
        raise AssertionError("canonical script/profile/depth inputs drifted")
    expected_force = "${{ (github.event_name == 'workflow_dispatch' && inputs.force_target == 'github') || (github.event_name == 'pull_request' && (github.event.pull_request.head.repo.full_name != github.repository || github.event.pull_request.user.type == 'Bot' || startsWith(github.event.pull_request.user.login, 'dependabot') || startsWith(github.event.pull_request.user.login, 'app/') || endsWith(github.event.pull_request.user.login, '[bot]'))) }}"
    if inputs.get("force_hosted") != expected_force:
        raise AssertionError("forced hosted/fork/bot isolation drifted")
    if call.get("if") != "github.event.pull_request.draft != true || github.event_name != 'pull_request'":
        raise AssertionError("draft proof admission drifted")
    active_lines = active_code_lines(proof_text)
    active = "\n".join(active_lines)
    if "set -euo pipefail" not in active_lines:
        raise AssertionError("proof script must propagate failures")
    if canonical_invocation_count(proof_text) != 1:
        raise AssertionError(".ci/rust-standard-proof.sh must invoke canonical proof exactly once")
    for fragment in LEGACY_INLINE_FRAGMENTS:
        if fragment in active:
            raise AssertionError(f".ci/rust-standard-proof.sh reintroduced semantic-step-body {fragment!r}")
    for line in active_lines:
        if line.strip().startswith("cargo ") and line.strip() not in ALLOWED_CARGO_COMMANDS:
            raise AssertionError(f".ci/rust-standard-proof.sh cargo-allowlist rejected {line.strip()!r}")
    aggregate = jobs.get("rust-small-result", {})
    if aggregate.get("needs") != "rust-small-proof" or aggregate.get("name") != "Perl LSP Rust Small Result" or aggregate.get("if") != "always()":
        raise AssertionError("required aggregate identity/dependency drifted")
    result_text = job_bodies(workflow_text).get("rust-small-result", "")
    if CONTRACT_TEST_FILE not in "\n".join(active_code_lines(result_text)):
        raise AssertionError(f"required aggregate must run {CONTRACT_TEST_FILE}")
    if set(jobs) != {"rust-small-proof", "rust-small-result"}:
        raise AssertionError("consumer must not retain ungoverned proof routes")


class RustStandardProofScriptTests(unittest.TestCase):
    """Every governed route executes this strict canonical proof entry point."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.log = self.directory / "cargo.log"
        self.config = self.directory / "gitconfig"
        binary = self.directory / "cargo"
        binary.write_text(
            '#!/usr/bin/env bash\n'
            'printf "%s\\n" "$*" >> "$PROOF_TEST_LOG"\n'
            'if [ "$1" = fmt ]; then exit "${FMT_STATUS:-0}"; fi\n'
            'exit "${PROOF_STATUS:-0}"\n',
            encoding="utf-8",
        )
        binary.chmod(0o755)
        self.environment = {
            **os.environ,
            "PATH": f"{self.directory}{os.pathsep}{os.environ['PATH']}",
            "GIT_CONFIG_GLOBAL": str(self.config),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GITHUB_WORKSPACE": str(self.directory / "workspace with spaces"),
            "PROOF_TEST_LOG": str(self.log),
        }
        for variable in subprocess.check_output(
            ["git", "rev-parse", "--local-env-vars"], text=True
        ).splitlines():
            self.environment.pop(variable, None)

    def run_script(
        self, *, cwd: Path | None = None, **environment: str
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(ROOT / ".ci/rust-standard-proof.sh")],
            cwd=cwd,
            env={**self.environment, **environment},
            capture_output=True,
            text=True,
            check=False,
        )

    def test_repeated_runs_preserve_canonical_proof_and_one_safe_directory(self) -> None:
        subprocess.run(
            ["git", "config", "--global", "--add", "safe.directory", "/unrelated/workspace"],
            env=self.environment,
            check=True,
        )
        for _ in range(2):
            result = self.run_script()
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            self.log.read_text().splitlines(),
            ["fmt --all -- --check", "run -p xtask --locked -- rust-small-proof"] * 2,
        )
        safe_directories = subprocess.check_output(
            ["git", "config", "--global", "--get-all", "safe.directory"],
            env=self.environment,
            text=True,
        ).splitlines()
        self.assertEqual(
            safe_directories, ["/unrelated/workspace", self.environment["GITHUB_WORKSPACE"]]
        )

    def test_format_failure_prevents_proof(self) -> None:
        result = self.run_script(FMT_STATUS="17")
        self.assertEqual(result.returncode, 17, result.stderr)
        self.assertEqual(self.log.read_text().splitlines(), ["fmt --all -- --check"])

    def test_proof_failure_propagates(self) -> None:
        result = self.run_script(PROOF_STATUS="23")
        self.assertEqual(result.returncode, 23, result.stderr)

    def test_safe_directory_write_failure_prevents_cargo(self) -> None:
        result = self.run_script(GIT_CONFIG_GLOBAL=str(self.directory / "absent/config"))
        self.assertNotEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.log.exists())

    def test_inherited_git_hook_environment_cannot_redirect_proof(self) -> None:
        workspace = Path(self.environment["GITHUB_WORKSPACE"])
        foreign = self.directory / "foreign"
        for repository in (workspace, foreign):
            subprocess.run(
                ["git", "init", "--quiet", str(repository)],
                env=self.environment,
                check=True,
            )
        observed = self.directory / "observed-repository"
        (self.directory / "cargo").write_text(
            '#!/usr/bin/env bash\n'
            'set -euo pipefail\n'
            'if [ "$1" = run ]; then\n'
            '  git rev-parse --show-toplevel > "$PROOF_TEST_REPOSITORY"\n'
            'fi\n',
            encoding="utf-8",
        )
        result = self.run_script(
            cwd=workspace,
            PROOF_TEST_REPOSITORY=str(observed),
            GIT_DIR=str(foreign / ".git"),
            GIT_WORK_TREE=str(foreign),
            GIT_COMMON_DIR=str(foreign / ".git"),
            GIT_INDEX_FILE=str(foreign / ".git/index"),
            GIT_OBJECT_DIRECTORY=str(foreign / ".git/objects"),
            GIT_ALTERNATE_OBJECT_DIRECTORIES=str(foreign / ".git/objects"),
            GIT_PREFIX="foreign/",
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(Path(observed.read_text().strip()), workspace)

    def test_inherited_git_config_cannot_disable_diff_hygiene(self) -> None:
        workspace = Path(self.environment["GITHUB_WORKSPACE"])
        subprocess.run(
            ["git", "init", "--quiet", str(workspace)], env=self.environment, check=True
        )
        tracked = workspace / "tracked.txt"
        tracked.write_text("clean\n", encoding="utf-8")
        subprocess.run(
            ["git", "-C", str(workspace), "add", "tracked.txt"],
            env=self.environment,
            check=True,
        )
        tracked.write_text("trailing whitespace \n", encoding="utf-8")
        (self.directory / "cargo").write_text(
            '#!/usr/bin/env bash\n'
            'if [ "$1" = run ]; then git diff --check; fi\n',
            encoding="utf-8",
        )
        result = self.run_script(
            cwd=workspace,
            GIT_CONFIG_COUNT="1",
            GIT_CONFIG_KEY_0="core.whitespace",
            GIT_CONFIG_VALUE_0="-trailing-space",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("trailing whitespace", result.stdout)


class RustSmallRouteContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow_text = load_workflow_text()
        self.proof_text = PROOF_PATH.read_text(encoding="utf-8")

    def test_checked_in_workflow_matches_canonical_route(self) -> None:
        validate_rust_small_route_contract(self.workflow_text, self.proof_text)

    def test_canonical_invocation_mutations_fail_named_site(self) -> None:
        for replacement in (CANONICAL_INVOCATION.replace(" --locked", ""), f"# {CANONICAL_INVOCATION}", f'echo "{CANONICAL_INVOCATION}"', f"{CANONICAL_INVOCATION} || true", ""):
            with self.subTest(replacement=replacement), self.assertRaisesRegex(AssertionError, "rust-standard-proof.sh"):
                validate_rust_small_route_contract(self.workflow_text, self.proof_text.replace(CANONICAL_INVOCATION, replacement))

    def test_shadow_duplicate_fails_closed(self) -> None:
        with self.assertRaisesRegex(AssertionError, "exactly once"):
            validate_rust_small_route_contract(self.workflow_text, self.proof_text + "\n" + CANONICAL_INVOCATION)

    def test_reintroduced_semantic_steps_and_counters_fail_closed(self) -> None:
        for extra in LEGACY_INLINE_FRAGMENTS:
            with self.subTest(extra=extra), self.assertRaisesRegex(AssertionError, "semantic-step-body"):
                validate_rust_small_route_contract(self.workflow_text, self.proof_text + "\n" + extra)

    def test_extra_semantic_cargo_command_fails_closed(self) -> None:
        for extra in ("cargo test --locked -p perl-lsp-rs --test route_parity_extra", "cargo clippy --workspace --locked -- -D warnings", "cargo run -p xtask --locked -- some-other-proof"):
            with self.subTest(extra=extra), self.assertRaisesRegex(AssertionError, "cargo-allowlist"):
                validate_rust_small_route_contract(self.workflow_text, self.proof_text + "\n" + extra)

    def test_strict_failure_propagation_cannot_be_removed(self) -> None:
        with self.assertRaisesRegex(AssertionError, "propagate failures"):
            validate_rust_small_route_contract(self.workflow_text, self.proof_text.replace("set -euo pipefail", "set +e"))

    def test_caller_mutations_fail_closed(self) -> None:
        for old, new in (("profile: standard", "profile: heavy"), ("script: .ci/rust-standard-proof.sh", "script: .ci/other.sh"), ("fetch_depth: 1", "fetch_depth: 0"), (".github/workflows/rust.yml@92c170b052d046c3c98acf8a1a4aab4510dda42d", ".github/workflows/rust.yml@main"), ("inputs.force_target == 'github'", "inputs.force_target == 'auto'"), (" || endsWith(github.event.pull_request.user.login, '[bot]')", ""), ("    with:\n", "    secrets: inherit\n    with:\n"), ("    if: github.event.pull_request.draft != true", "    if: github.event.pull_request.draft == true"), ("  rust-small-proof:\n", "  removed:\n")):
            with self.subTest(old=old), self.assertRaises(AssertionError):
                broken = self.workflow_text.replace(old, new, 1)
                self.assertNotEqual(broken, self.workflow_text)
                validate_rust_small_route_contract(broken, self.proof_text)

    def test_aggregate_identity_and_dependencies_remain_required(self) -> None:
        for old, new in (("name: Perl LSP Rust Small Result", "name: CI Result"), ("needs: rust-small-proof", "needs: []"), ("if: always()", "if: false")):
            with self.subTest(old=old), self.assertRaises(AssertionError):
                validate_rust_small_route_contract(self.workflow_text.replace(old, new, 1), self.proof_text)

    def test_result_job_without_contract_reference_fails_closed(self) -> None:
        with self.assertRaisesRegex(AssertionError, "must run"):
            validate_rust_small_route_contract(self.workflow_text.replace(CONTRACT_TEST_FILE, "scripts/ci/removed.py"), self.proof_text)


if __name__ == "__main__":
    unittest.main()
