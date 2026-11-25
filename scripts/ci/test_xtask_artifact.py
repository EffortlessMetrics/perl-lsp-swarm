"""Exercise the real executable handoff and its hosted workflow consumers.

The fixture executable tests custody and refusal, not Rust behavior. Actual
compile and hosted timing remain the CI run's responsibility.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest

from scripts.ci.evaluate_ci_gate import evaluate
from scripts.ci.validate_gate_enforcement_contract import _constant_truth


ROOT = Path(__file__).resolve().parents[2]
HELPER = ROOT / "scripts/ci/xtask_artifact.py"


def job(text: str, name: str) -> str:
    match = re.search(rf"^  {re.escape(name)}:\n(.*?)(?=^  [\w-]+:|\Z)", text, re.M | re.S)
    if not match:
        raise ValueError(f"missing job: {name}")
    return match.group(1)


def step(text: str, name: str) -> str:
    match = re.search(
        rf"^      - name: {re.escape(name)}\n(.*?)(?=^      - |\Z)", text, re.M | re.S
    )
    if not match:
        raise ValueError(f"missing step: {name}")
    return match.group(1)


class ArtifactTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update(
            GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
            GIT_AUTHOR_NAME="Fixture", GIT_COMMITTER_NAME="Fixture",
            GIT_AUTHOR_EMAIL="fixture@example.invalid", GIT_COMMITTER_EMAIL="fixture@example.invalid",
            GITHUB_REPOSITORY="Fixture/repo", GITHUB_RUN_ID="42", GITHUB_RUN_ATTEMPT="3",
            GITHUB_ENV=str(self.root / "github-env"),
        )
        self.git("init", "-q", "--template=")
        (self.repo / "source").write_text("source-one\n")
        self.git("add", "source")
        self.git("commit", "-qm", "fixture")
        self.env["GITHUB_SHA"] = self.git("rev-parse", "HEAD").strip()
        tools = self.root / "tools"
        tools.mkdir()
        rustc = tools / "rustc"
        rustc.write_text(f"#!{sys.executable}\nimport os\nprint(os.environ.get('TEST_RUSTC', 'rustc fixture; host: x86_64-unknown-linux-gnu'))\n")
        rustc.chmod(0o700)
        self.env["PATH"] = str(tools) + os.pathsep + self.env["PATH"]
        binary = self.repo / "target/debug/xtask"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"#!/bin/sh\nprintf 'fixture executable ran\\n'\n")
        binary.chmod(0o700)
        self.artifact = self.root / "artifact"
        self.destination = self.root / "verified"

    def git(self, *args: str) -> str:
        return subprocess.check_output(["git", *args], cwd=self.repo, env=self.env, text=True)

    def run_helper(self, command: str) -> subprocess.CompletedProcess[str]:
        args = [sys.executable, str(HELPER), command, "--artifact-dir", str(self.artifact)]
        if command == "install":
            args += ["--destination", str(self.destination)]
        return subprocess.run(args, cwd=self.repo, env=self.env, text=True, capture_output=True)

    def pack(self) -> None:
        result = self.run_helper("pack")
        self.assertEqual(0, result.returncode, result.stderr)

    def mutate_manifest(self, name: str, value: object) -> None:
        path = self.artifact / "manifest.json"
        manifest = json.loads(path.read_text())
        manifest[name] = value
        path.write_text(json.dumps(manifest))

    def assert_refused(self, reason: str) -> None:
        result = self.run_helper("install")
        self.assertNotEqual(0, result.returncode, result.stdout)
        self.assertIn(reason, result.stderr)
        self.assertFalse(self.destination.exists())
        self.assertFalse(Path(self.env["GITHUB_ENV"]).exists())

    def test_same_subject_runs_the_copied_executable_outside_target(self) -> None:
        self.pack()
        self.assertEqual({"xtask", "manifest.json"}, {p.name for p in self.artifact.iterdir()})
        result = self.run_helper("install")
        self.assertEqual(0, result.returncode, result.stderr)
        binary = self.destination / "xtask"
        self.assertEqual(f"CI_XTASK={binary}\n", Path(self.env["GITHUB_ENV"]).read_text())
        # A later Cargo build can replace target/debug without replacing the
        # verified executable selected by run_gate_shard.py.
        (self.repo / "target/debug/xtask").write_text("replacement build")
        self.assertEqual("fixture executable ran\n", subprocess.check_output([str(binary)], text=True))
        self.assertEqual(0o500, binary.stat().st_mode & 0o777)

    def test_each_identity_dimension_refuses_a_different_producer(self) -> None:
        self.pack()
        original = (self.artifact / "manifest.json").read_bytes()
        for name, value in (
            ("source_sha", "0" * 40), ("tree_sha", "1" * 40),
            ("run_id", "41"), ("run_attempt", "4"), ("repository", "Other/repo"),
            ("rustc", "different compiler"), ("system", "other system"),
            ("machine", "other architecture"), ("schema_version", True),
            ("workspace", "/another/checkout"),
        ):
            with self.subTest(name=name):
                (self.artifact / "manifest.json").write_bytes(original)
                self.mutate_manifest(name, value)
                self.assert_refused(name)

    def test_failed_job_rerun_reuses_exact_same_run_producer(self) -> None:
        self.pack()
        self.env["GITHUB_RUN_ATTEMPT"] = "4"
        result = self.run_helper("install")
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(
            "fixture executable ran\n",
            subprocess.check_output([str(self.destination / "xtask")], text=True),
        )

    def test_invalid_or_future_producer_attempt_and_other_run_refuse(self) -> None:
        self.pack()
        original = (self.artifact / "manifest.json").read_bytes()
        for attempt in (None, True, 0, "", "0", "-1", "2.0", "4"):
            with self.subTest(attempt=attempt):
                (self.artifact / "manifest.json").write_bytes(original)
                self.mutate_manifest("run_attempt", attempt)
                self.assert_refused("run_attempt")
        (self.artifact / "manifest.json").write_bytes(original)
        self.env["GITHUB_RUN_ID"] = "43"
        self.assert_refused("run_id")

    def test_changed_bytes_and_missing_members_refuse_without_fallback_build(self) -> None:
        self.pack()
        binary = self.artifact / "xtask"
        original = binary.read_bytes()
        binary.write_bytes(original + b"changed")
        self.assert_refused("digest or size")
        binary.unlink()
        self.assert_refused("exactly")

    def test_malformed_manifest_and_extra_artifact_member_refuse(self) -> None:
        self.pack()
        manifest = self.artifact / "manifest.json"
        original = manifest.read_bytes()
        for payload in (b"not-json", b"[]", b"{}"):
            manifest.write_bytes(payload)
            result = self.run_helper("install")
            self.assertNotEqual(0, result.returncode)
            self.assertFalse(self.destination.exists())
        manifest.write_bytes(original)
        (self.artifact / "extra").write_text("foreign result")
        self.assert_refused("exactly")

    def test_linked_member_is_not_executed(self) -> None:
        self.pack()
        binary = self.artifact / "xtask"
        outside = self.root / "outside"
        binary.rename(outside)
        binary.symlink_to(outside)
        self.assert_refused("linked artifact")

    def test_dirty_or_wrong_checkout_cannot_export_a_matching_identity(self) -> None:
        self.env["GITHUB_SHA"] = "0" * 40
        result = self.run_helper("pack")
        self.assertNotEqual(0, result.returncode)
        self.assertIn("checkout differs", result.stderr)
        self.assertFalse(self.artifact.exists())
        self.env["GITHUB_SHA"] = self.git("rev-parse", "HEAD").strip()
        (self.repo / "source").write_text("dirty source\n")
        result = self.run_helper("pack")
        self.assertNotEqual(0, result.returncode)
        self.assertFalse(self.artifact.exists())

    def test_real_consumer_compiler_change_refuses(self) -> None:
        self.pack()
        self.env["TEST_RUSTC"] = "rustc different"
        self.assert_refused("rustc")

    def test_identical_commit_in_a_different_checkout_path_refuses(self) -> None:
        self.pack()
        relocated = self.root / "relocated-repo"
        self.repo.rename(relocated)
        self.repo = relocated
        self.assert_refused("workspace")

    def test_existing_destination_is_never_replaced(self) -> None:
        self.pack()
        self.destination.mkdir()
        sentinel = self.destination / "xtask"
        sentinel.write_text("keep existing executable")
        result = self.run_helper("install")
        self.assertNotEqual(0, result.returncode)
        self.assertEqual("keep existing executable", sentinel.read_text())
        self.assertFalse(Path(self.env["GITHUB_ENV"]).exists())


class PrebuiltConsumerTests(unittest.TestCase):
    """Reach the real shell seams without permitting Cargo compilation."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / "repo"
        (self.repo / "ci").mkdir(parents=True)
        (self.repo / "xtask/src").mkdir(parents=True)
        for source in ("ci/xtask_wrapper.sh", "ci/check_unsafe_prod.sh", "rust-toolchain.toml"):
            shutil.copyfile(ROOT / source, self.repo / source)
        for source in ("Cargo.toml", "Cargo.lock", "xtask/Cargo.toml"):
            (self.repo / source).touch()
        self.calls = self.root / "calls.jsonl"
        self.env = {
            key: value for key, value in os.environ.items()
            if not key.startswith(("GIT_", "CARGO_", "RUSTUP_")) and key != "CI_XTASK"
        }
        tools = self.root / "tools"
        tools.mkdir()
        self.env.update(PATH=str(tools) + os.pathsep + self.env["PATH"], CALLS=str(self.calls))
        for name in ("cargo", "rustup"):
            self.executable(tools / name, name, 86)
        self.verified = self.root / "verified xtask ' ; $literal"
        self.executable(self.verified, "verified", 0)
        self.env["CI_XTASK"] = str(self.verified)
        self.local = self.repo / "target/debug/xtask"
        self.executable(self.local, "local", 0)
        os.utime(self.local, (1, 1))

    def executable(self, path: Path, label: str, status: int) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(
            f"#!{sys.executable}\n"
            "import json, os, sys\n"
            "with open(os.environ['CALLS'], 'a') as log:\n"
            f"    log.write(json.dumps([{label!r}, os.getcwd(), sys.argv[1:]]) + '\\n')\n"
            f"sys.exit({status})\n"
        )
        path.chmod(0o700)

    def recorded(self) -> list[list[object]]:
        if not self.calls.exists():
            return []
        return [json.loads(line) for line in self.calls.read_text().splitlines()]

    def release_check(self) -> subprocess.CompletedProcess[str]:
        # Extract the owning function, including its real fallback ladder.
        # The surrounding release checks stay in their original script.
        source = (ROOT / "scripts/check_release_history.sh").read_text()
        function = "run_install_surface_check() {\n" + source.split(
            "run_install_surface_check() {\n", 1
        )[1].split("\ncargo_guard_before_fallback() {", 1)[0]
        return subprocess.run(
            ["bash", "-eu", "-c", function + "\ncargo_guard_before_fallback() { return 0; }\nrun_install_surface_check"],
            cwd=self.repo, env=self.env, capture_output=True, text=True,
        )

    def unsafe_check(self, *args: str) -> subprocess.CompletedProcess[str]:
        # Run the real entrypoint from outside the repo; the wrapper must still
        # resolve its pinned toolchain and establish its original working root.
        return subprocess.run(
            ["bash", str(self.repo / "ci/check_unsafe_prod.sh"), *args],
            cwd=self.root, env=self.env, capture_output=True, text=True,
        )

    def test_release_prefers_verified_executable_over_stale_target(self) -> None:
        result = self.release_check()
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual([["verified", str(self.repo), ["install-surface-check"]]], self.recorded())

    def test_release_refuses_invalid_present_executable_without_local_or_cargo_fallback(self) -> None:
        linked = self.root / "linked-xtask"
        linked.symlink_to(self.verified)
        non_executable = self.root / "not-executable"
        non_executable.write_text("not executable")
        for path in ("", str(self.root / "missing"), str(linked), str(non_executable)):
            with self.subTest(path=path):
                self.env["CI_XTASK"] = path
                result = self.release_check()
                self.assertNotEqual(0, result.returncode)
                self.assertIn("CI_XTASK", result.stderr)
                self.assertEqual([], self.recorded())

    def test_release_preserves_verified_failure_without_fallback(self) -> None:
        self.executable(self.verified, "verified", 17)
        result = self.release_check()
        self.assertEqual(17, result.returncode, result.stderr)
        self.assertEqual([["verified", str(self.repo), ["install-surface-check"]]], self.recorded())

    def test_release_without_shared_executable_retains_local_prebuilt_fallback(self) -> None:
        self.env.pop("CI_XTASK")
        result = self.release_check()
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual([["local", str(self.repo), ["install-surface-check"]]], self.recorded())

    def test_unsafe_wrapper_keeps_repo_root_literal_arguments_and_exit_status(self) -> None:
        self.executable(self.verified, "verified", 17)
        argument = "value with ' quote; $literal"
        result = self.unsafe_check(argument)
        self.assertEqual(17, result.returncode, result.stderr)
        self.assertEqual(
            [["verified", str(self.repo), ["ci-hygiene", "check-unsafe-prod", argument]]],
            self.recorded(),
        )

    def test_unsafe_wrapper_refuses_invalid_present_executable_without_cargo_fallback(self) -> None:
        linked = self.root / "linked-xtask"
        linked.symlink_to(self.verified)
        for path in ("", str(self.root / "missing"), str(linked)):
            with self.subTest(path=path):
                self.env["CI_XTASK"] = path
                result = self.unsafe_check()
                self.assertNotEqual(0, result.returncode)
                self.assertIn("CI_XTASK", result.stderr)
                self.assertEqual([], self.recorded())

    def test_unsafe_wrapper_without_shared_executable_keeps_pinned_cargo(self) -> None:
        self.env.pop("CI_XTASK")
        result = self.unsafe_check("--fixture")
        self.assertEqual(86, result.returncode, result.stderr)
        self.assertEqual(
            [["rustup", str(self.repo), ["run", "1.100.0", "cargo", "xtask", "ci-hygiene", "check-unsafe-prod", "--fixture"]]],
            self.recorded(),
        )

    def test_package_wrapper_feature_selection_stays_real_cargo(self) -> None:
        result = subprocess.run(
            ["bash", "-c", 'source "$1"; ci_run_xtask_package check-ignored', "fixture", str(self.repo / "ci/xtask_wrapper.sh")],
            cwd=self.root, env=self.env, capture_output=True, text=True,
        )
        self.assertEqual(86, result.returncode, result.stderr)
        self.assertEqual(
            [["rustup", str(self.repo), ["run", "1.100.0", "cargo", "run", "--quiet", "--manifest-path", str(self.repo / "Cargo.toml"), "-p", "xtask", "--no-default-features", "--", "check-ignored"]]],
            self.recorded(),
        )


class WorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.workflow = (ROOT / ".github/workflows/ci.yml").read_text()

    def test_only_revival_automatically_runs_the_full_tier(self) -> None:
        smoke = job(self.workflow, "pr-smoke")
        condition = next(line.strip() for line in smoke.splitlines() if line.startswith("    if:"))
        self.assertEqual(
            "if: github.event_name == 'workflow_dispatch' && inputs.run_pr_fast == true && "
            "needs.draft-pr-check.outputs.run_ci == 'true' && needs.preflight-latest-check.outputs.is_latest == 'true'",
            condition,
        )
        self.assertRegex(self.workflow, r"run_pr_fast:\n(?: +[^\n]+\n)*? +default: false\n +type: boolean")
        self.assertIn("gates --tier pr-fast --subject", smoke)
        revival = job(self.workflow, "pr-fast-unblocked")
        condition = next(line.strip() for line in revival.splitlines() if line.startswith("    if:"))
        self.assertEqual(
            "if: needs.draft-pr-check.outputs.run_ci == 'true' && "
            "needs.preflight-latest-check.outputs.is_latest == 'true'",
            condition,
        )
        self.assertIn("gates --tier pr-fast --subject", revival)
        self.assertNotIn("Upload PR-fast test results to Codecov", smoke)
        self.assertIn("Upload PR-fast test results to Codecov", revival)
        self.assertEqual(1, revival.count("name: Convert PR-fast receipt to JUnit"))

    def test_bootstrap_binds_one_locked_binary_to_this_run_attempt(self) -> None:
        producer = job(self.workflow, "ci-xtask")
        self.assertIn("needs: [draft-pr-check, conflict-markers, preflight-latest-check, rust-formatting]", producer)
        self.assertIn("ref: ${{ github.sha }}", producer)
        self.assertIn("persist-credentials: false", producer)
        self.assertIn("toolchain: 1.100.0", producer)
        self.assertIn("save-if: false", step(producer, "Cache cargo dependencies"))
        warm = step(producer, "Warm xtask")
        self.assertEqual("run: cargo build -p xtask --bin xtask --locked", warm.strip())
        self.assertNotIn("continue-on-error:", producer)
        self.assertIn("run: python3 -m unittest scripts/ci/test_xtask_artifact.py", producer)
        self.assertLess(producer.index("Verify shared xtask handoff"), producer.index("Warm xtask"))
        self.assertIn("python3 scripts/ci/verify_shared_xtask_execution.py --xtask target/debug/xtask", producer)
        self.assertLess(producer.index("Warm xtask"), producer.index("Verify shared xtask execution"))
        self.assertLess(producer.index("Verify shared xtask execution"), producer.index("Bind shared xtask"))
        upload = step(producer, "Upload shared xtask")
        self.assertIn("id: upload", upload)
        self.assertIn("ci-xtask-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.sha }}", upload)
        self.assertIn("if-no-files-found: error", upload)
        self.assertIn("retention-days: 7", upload)
        self.assertIn("artifact_id: ${{ steps.upload.outputs.artifact-id }}", producer)

    def test_shards_consume_exact_artifact_without_another_unconditional_build(self) -> None:
        shards = job(self.workflow, "merge-gate-shards")
        self.assertIn("needs: [draft-pr-check, conflict-markers, preflight-latest-check, ci-xtask]", shards)
        download = step(shards, "Download shared xtask")
        self.assertIn("artifact-ids: ${{ needs.ci-xtask.outputs.artifact_id }}", download)
        self.assertIn("merge-multiple: true", download)
        self.assertNotIn("github-token:", download)
        self.assertNotIn("run-id:", download)
        install = step(shards, "Warm xtask")
        self.assertIn("python3 scripts/ci/xtask_artifact.py install", install)
        self.assertNotIn("continue-on-error:", install)
        self.assertNotIn("if:", install)
        self.assertNotIn("cargo build -p xtask", shards)
        self.assertIn('--xtask "$CI_XTASK"', shards)
        self.assertIn('"$CI_XTASK" gate-failure-digest', shards)
        self.assertLess(shards.index("Validate shared xtask artifact ID"), shards.index("Download shared xtask"))
        self.assertLess(shards.index("Download shared xtask"), shards.index("Warm xtask"))
        self.assertLess(shards.index("Warm xtask"), shards.index("Run merge-gate shard with receipts"))

    def test_bootstrap_requires_same_subject_prerequisites_without_pr_head_coupling(self) -> None:
        producer = job(self.workflow, "ci-xtask")
        match = re.search(r"^    if: >-\n((?:      [^\n]*\n)+)", producer, re.M)
        self.assertIsNotNone(match)
        condition = " ".join(line.strip() for line in match.group(1).splitlines())
        self.assertIn("!cancelled()", condition, "explicit status check must override implicit needs-success")
        for event, conflict, formatting, draft_result, latest_result, run_ci, latest, cancelled, expected in (
            ("pull_request", "success", "failure", "success", "success", "true", "true", False, True),
            ("merge_group", "success", "success", "success", "success", "true", "true", False, True),
            ("merge_group", "success", "failure", "success", "success", "true", "true", False, False),
            ("push", "success", "failure", "success", "success", "true", "true", False, False),
            ("workflow_dispatch", "success", "failure", "success", "success", "true", "true", False, False),
            ("pull_request", "failure", "success", "success", "success", "true", "true", False, False),
            ("pull_request", "success", "success", "failure", "success", "true", "true", False, False),
            ("pull_request", "success", "success", "success", "failure", "true", "true", False, False),
            ("pull_request", "success", "success", "success", "success", "false", "true", False, False),
            ("pull_request", "success", "success", "success", "success", "true", "false", False, False),
            ("pull_request", "success", "success", "success", "success", "true", "true", True, False),
        ):
            with self.subTest(event=event, conflict=conflict, formatting=formatting, cancelled=cancelled):
                values = {
                    "github.event_name": event,
                    "needs.conflict-markers.result": conflict,
                    "needs.rust-formatting.result": formatting,
                    "needs.draft-pr-check.result": draft_result,
                    "needs.preflight-latest-check.result": latest_result,
                    "needs.draft-pr-check.outputs.run_ci": run_ci,
                    "needs.preflight-latest-check.outputs.is_latest": latest,
                }
                expression = condition.replace("!cancelled()", "false" if cancelled else "true")
                expression = re.sub(
                    r"([A-Za-z_.-]+) == '([^']*)'",
                    lambda found: "true" if values[found.group(1)] == found.group(2) else "false",
                    expression,
                )
                self.assertIs(expected, _constant_truth(expression), expression)

    def test_missing_or_multiple_artifact_ids_cannot_select_all_artifacts(self) -> None:
        guard = step(job(self.workflow, "merge-gate-shards"), "Validate shared xtask artifact ID")
        self.assertIn("XTASK_ARTIFACT_ID: ${{ needs.ci-xtask.outputs.artifact_id }}", guard)
        self.assertNotIn("continue-on-error:", guard)
        self.assertNotIn("if:", guard)
        script = textwrap.dedent(guard.split("run: |\n", 1)[1])
        for artifact_id in ("77", "", "0", "missing", "77,78", "77\n78"):
            with self.subTest(artifact_id=artifact_id):
                result = subprocess.run(
                    ["bash", "-c", script], env={**os.environ, "XTASK_ARTIFACT_ID": artifact_id},
                    capture_output=True, text=True,
                )
                self.assertEqual(artifact_id == "77", result.returncode == 0, result.stdout)

    def test_required_compile_refuses_failed_or_missing_cheap_proof_before_checkout(self) -> None:
        compile_job = job(self.workflow, "check-all-targets")
        condition = next(line.strip() for line in compile_job.splitlines() if line.startswith("    if:"))
        self.assertEqual(
            "if: ${{ !cancelled() && needs.draft-pr-check.outputs.run_ci == 'true' && "
            "needs.preflight-latest-check.outputs.is_latest == 'true' }}",
            condition,
        )
        for cancelled, run_ci, latest, expected in (
            (False, True, True, True),
            (True, True, True, False),
            (False, False, True, False),
            (False, True, False, False),
        ):
            expression = condition.removeprefix("if: ${{ ").removesuffix(" }}")
            expression = expression.replace("!cancelled()", "false" if cancelled else "true")
            expression = expression.replace(
                "needs.draft-pr-check.outputs.run_ci == 'true'", str(run_ci).lower()
            ).replace(
                "needs.preflight-latest-check.outputs.is_latest == 'true'", str(latest).lower()
            )
            self.assertIs(expected, _constant_truth(expression), expression)
        self.assertIn("needs: [draft-pr-check, preflight-latest-check, conflict-markers, rust-formatting]", compile_job)
        guard = step(compile_job, "Require cheap prerequisite proof")
        self.assertIn("CONFLICT_RESULT: ${{ needs.conflict-markers.result }}", guard)
        self.assertIn("FORMAT_RESULT: ${{ needs.rust-formatting.result }}", guard)
        self.assertIn("EVENT_NAME: ${{ github.event_name }}", guard)
        self.assertNotIn("continue-on-error:", compile_job)
        self.assertNotIn("if:", guard)
        self.assertLess(compile_job.index("Require cheap prerequisite proof"), compile_job.index("uses: actions/checkout@"))
        script = textwrap.dedent(guard.split("run: |\n", 1)[1]).split("# INTENTIONAL:", 1)[0]
        for event, conflict, formatting, expected in (
            ("merge_group", "success", "success", True),
            ("merge_group", "failure", "success", False),
            ("merge_group", "success", "failure", False),
            ("push", "success", "failure", False),
            ("workflow_dispatch", "success", "failure", False),
            ("pull_request", "success", "failure", True),
            ("pull_request", "failure", "success", False),
            ("merge_group", "skipped", "success", False),
            ("merge_group", "success", "cancelled", False),
            ("merge_group", "", "success", False),
        ):
            with self.subTest(event=event, conflict=conflict, formatting=formatting):
                result = subprocess.run(
                    ["bash", "-c", script],
                    env={**os.environ, "CONFLICT_RESULT": conflict, "FORMAT_RESULT": formatting, "EVENT_NAME": event},
                    capture_output=True, text=True,
                )
                self.assertEqual(expected, result.returncode == 0, result.stdout)

    def test_missing_bootstrap_cannot_produce_green_aggregate_from_skipped_shards(self) -> None:
        needs = {name: {"result": "success", "outputs": {}} for name in (
            "draft-pr-check", "preflight-latest-check", "conflict-markers", "check-all-targets",
            "ux-tests", "merge-gate-shards",
        )}
        needs["draft-pr-check"]["outputs"] = {"run_ci": "true"}
        needs["preflight-latest-check"]["outputs"] = {"is_latest": "true"}
        self.assertEqual("success", evaluate(needs, event_name="merge_group").status)
        for outcome in ("skipped", "failure", "cancelled"):
            needs["merge-gate-shards"]["result"] = outcome
            self.assertNotEqual("success", evaluate(needs, event_name="merge_group").status)


if __name__ == "__main__":
    unittest.main()
