#!/usr/bin/env python3
"""Execute the PR Smoke workflow's version-pairing seam without compiling Rust."""

from __future__ import annotations

import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/ci.yml"
HELPER = "scripts/ci/pr_smoke_disk_guard.py"
CURRENT_HELPER = (ROOT / HELPER).read_text(encoding="utf-8")
GIB = 1024**3
# Model the historical CLI contract: the candidate accepts min-free-bytes,
# but rejects the workflow's newer footprint argument before doing any work.
LEGACY_HELPER = """import argparse
p = argparse.ArgumentParser()
s = p.add_subparsers(dest='mode', required=True)
f = s.add_parser('preflight')
f.add_argument('--workspace-path', default='.')
f.add_argument('--target-dir', default='target')
f.add_argument('--min-free-bytes', default='32GiB')
s.add_parser('classify')
p.parse_args()
"""


def smoke_job() -> str:
    text = WORKFLOW.read_text(encoding="utf-8")
    job = re.search(r"^  pr-smoke:\n(.*?)(?=^  [\w-]+:|\Z)", text, re.M | re.S)
    assert job, "PR Smoke job missing"
    return job.group(1)


def smoke_step(name: str) -> tuple[str, str]:
    """Read the actual Bash body and metadata, without a YAML dependency."""
    match = re.search(
        rf"^      - name: {re.escape(name)}\n(.*?)(?=^      - |\Z)",
        smoke_job(), re.M | re.S,
    )
    assert match, f"workflow step missing: {name}"
    metadata, body = match.group(1).split("        run: |\n", 1)
    lines = body.splitlines()
    script = []
    for line in lines:
        if line and not line.startswith("          "):
            break
        script.append(line[10:] if line else "")
    return metadata, "\n".join(script) + "\n"


class WorkflowGuardTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.repo = self.root / "origin"
        self.repo.mkdir()
        self.env = os.environ.copy()
        self.env.update({
            "GIT_AUTHOR_NAME": "Fixture", "GIT_COMMITTER_NAME": "Fixture",
            "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
            "GIT_COMMITTER_EMAIL": "fixture@example.invalid",
            "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
        })
        self.git("init", "-q", "-b", "candidate")
        self.write_helper(LEGACY_HELPER)
        self.commit("legacy candidate")
        self.candidate_sha = self.git("rev-parse", "HEAD").stdout.strip()
        self.git("checkout", "-q", "-b", "workflow")
        self.write_helper(CURRENT_HELPER)
        self.commit("workflow helper")
        self.workflow_sha = self.git("rev-parse", "HEAD").stdout.strip()
        self.git("checkout", "-q", "-b", "main")
        self.write_helper("raise SystemExit('wrong newer main helper')\n")
        self.commit("divergent main")
        self.git("checkout", "-q", "candidate")
        self.runner_temp = self.root / "runner-temp"
        self.runner_temp.mkdir()
        self.env_file = self.root / "github-env"
        self.env.update({
            "PR_SMOKE_WORKFLOW_SHA": self.workflow_sha,
            "RUNNER_TEMP": str(self.runner_temp), "GITHUB_ENV": str(self.env_file),
            "CARGO_TARGET_DIR": "target", "PR_SMOKE_FOOTPRINT_GIB": "85",
        })
        # Test-only probe injection exercises the real CLI at exact capacities.
        probes = self.root / "probes"
        probes.mkdir()
        (probes / "sitecustomize.py").write_text(
            "import os, shutil\n"
            "free = int(os.environ['TEST_FREE_BYTES'])\n"
            "shutil.disk_usage = lambda path: shutil._ntuple_diskusage(200*1024**3, "
            "200*1024**3-free, free)\n", encoding="utf-8",
        )
        self.env.update({"PYTHONPATH": str(probes), "TEST_FREE_BYTES": str(90 * GIB)})

    def git(self, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["git", *args], cwd=self.repo, env=self.env,
            text=True, capture_output=True, check=check,
        )

    def write_helper(self, source: str) -> None:
        path = self.repo / HELPER
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source, encoding="utf-8")

    def commit(self, message: str) -> None:
        self.git("add", "--", HELPER)
        self.git("commit", "-qm", message)

    def run_step(self, name: str) -> subprocess.CompletedProcess[str]:
        _, script = smoke_step(name)
        return subprocess.run(
            ["bash", "-e", "-c", script], cwd=self.repo, env=self.env,
            text=True, capture_output=True,
        )

    def materialize(self) -> subprocess.CompletedProcess[str]:
        result = self.run_step("Resolve workflow-bound PR Smoke disk guard")
        if result.returncode == 0:
            for line in self.env_file.read_text(encoding="utf-8").splitlines():
                key, value = line.split("=", 1)
                self.env[key] = value
        return result

    def assert_candidate_unchanged(self) -> None:
        self.assertEqual(self.git("rev-parse", "HEAD").stdout.strip(), self.candidate_sha)
        self.assertEqual(self.git("diff", "--", HELPER).stdout, "")
        self.assertEqual((self.repo / HELPER).read_text(encoding="utf-8"), LEGACY_HELPER)

    def test_legacy_candidate_rejects_but_workflow_pair_enforces_exact_budget(self) -> None:
        unsupported = subprocess.run(
            ["python3", HELPER, "preflight", "--footprint-bytes", "85GiB"],
            cwd=self.repo, env=self.env, text=True, capture_output=True,
        )
        self.assertEqual(unsupported.returncode, 2)
        self.assertIn("unrecognized arguments", unsupported.stderr)
        result = self.materialize()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(Path(self.env["PR_SMOKE_DISK_GUARD"]).read_text(), CURRENT_HELPER)
        for free in (86 * GIB, 90 * GIB - 1):
            self.env["TEST_FREE_BYTES"] = str(free)
            refused = self.run_step("Pre-flight disk budget guard")
            self.assertEqual(refused.returncode, 1, refused.stdout + refused.stderr)
            self.assertIn(f"budget {90 * GIB} bytes", refused.stdout)
        self.env["TEST_FREE_BYTES"] = str(90 * GIB)
        accepted = self.run_step("Pre-flight disk budget guard")
        self.assertEqual(accepted.returncode, 0, accepted.stdout + accepted.stderr)
        self.assertIn(str(90 * GIB), accepted.stdout)
        self.assert_candidate_unchanged()

    def test_both_calls_work_when_candidate_helper_is_absent(self) -> None:
        (self.repo / HELPER).unlink()
        result = self.materialize()
        self.assertEqual(result.returncode, 0, result.stderr)
        result = self.run_step("Pre-flight disk budget guard")
        self.assertEqual(result.returncode, 0, result.stderr)
        logs = self.repo / "target/receipts/logs"
        (logs / "current.log").write_text("No space left on device\n", encoding="utf-8")
        result = self.run_step("Classify PR-fast disk exhaustion")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("resource-exhaustion", result.stdout)
        self.assertIn("ENOSPC", (logs / "pr-fast-disk-pressure.log").read_text())
        self.assertFalse((self.repo / HELPER).exists())
        self.assertEqual(self.git("rev-parse", "HEAD").stdout.strip(), self.candidate_sha)

    def test_declared_footprint_argument_cannot_be_silently_dropped(self) -> None:
        result = self.materialize()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.env["PR_SMOKE_FOOTPRINT_GIB"] = "86"
        result = self.run_step("Pre-flight disk budget guard")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f"budget {91 * GIB} bytes", result.stdout)

    def test_missing_workflow_object_fetches_exact_sha_without_shallowing(self) -> None:
        clone = self.root / "candidate-clone"
        self.git("clone", "-q", "--no-local", "--single-branch", "--branch", "candidate",
                 str(self.repo), str(clone))
        self.repo = clone
        self.assertNotEqual(self.git("cat-file", "-e", self.workflow_sha, check=False).returncode, 0)
        result = self.materialize()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.git("rev-parse", "--is-shallow-repository").stdout.strip(), "false")
        self.assert_candidate_unchanged()

    def test_unavailable_commit_fails_closed_before_guard_is_exported(self) -> None:
        self.env["PR_SMOKE_WORKFLOW_SHA"] = "0" * 40
        self.git("remote", "add", "origin", str(self.repo))
        self.assertNotEqual(self.materialize().returncode, 0)
        self.assertFalse(self.env_file.exists())
        self.assertFalse((self.runner_temp / "pr-smoke-disk-guard.py").exists())
        self.assert_candidate_unchanged()

    def test_commit_without_helper_fails_closed(self) -> None:
        self.git("checkout", "-q", "workflow")
        self.git("rm", "-q", "--", HELPER)
        self.git("commit", "-qm", "missing guard")
        self.env["PR_SMOKE_WORKFLOW_SHA"] = self.git("rev-parse", "HEAD").stdout.strip()
        self.git("checkout", "-q", "candidate")
        self.assertNotEqual(self.materialize().returncode, 0)
        self.assertFalse(self.env_file.exists())
        self.assertEqual(list(self.runner_temp.iterdir()), [])
        self.assert_candidate_unchanged()

    def test_non_sha_source_fails_closed(self) -> None:
        self.env["PR_SMOKE_WORKFLOW_SHA"] = "main"
        result = self.materialize()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("exact workflow commit SHA", result.stdout)
        self.assertFalse(self.env_file.exists())

    def test_source_and_classifier_readiness_are_bound_in_workflow(self) -> None:
        self.assertRegex(smoke_job(), r'PR_SMOKE_FOOTPRINT_GIB: "85"')
        metadata, _ = smoke_step("Resolve workflow-bound PR Smoke disk guard")
        self.assertIn("PR_SMOKE_WORKFLOW_SHA: ${{ github.workflow_sha }}", metadata)
        metadata, _ = smoke_step("Classify PR-fast disk exhaustion")
        self.assertIn("always() && steps.pr-smoke-disk-guard.outcome == 'success'", metadata)


if __name__ == "__main__":
    unittest.main()
