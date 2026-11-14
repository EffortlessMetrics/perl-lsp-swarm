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
BASELINE = ".ci/pr-smoke-footprint-baseline.json"
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
    block = match.group(1)
    if "        run: |\n" in block:
        metadata, body = block.split("        run: |\n", 1)
        lines = body.splitlines()
        script = []
        for line in lines:
            if line and not line.startswith("          "):
                break
            script.append(line[10:] if line else "")
        return metadata, "\n".join(script) + "\n"
    return block, ""


def upload_retention_days() -> dict[str, int]:
    """Every actions/upload-artifact step in the workflow and its retention.

    Read from the raw workflow text, without a YAML dependency, so the
    assertion binds to the value GitHub Actions will actually honour.
    """
    text = WORKFLOW.read_text(encoding="utf-8")
    retentions: dict[str, int] = {}
    for match in re.finditer(
        r"^      - name: (?P<name>[^\n]+)\n(?P<body>.*?)(?=^      - |\Z)",
        text,
        re.M | re.S,
    ):
        body = match.group("body")
        if "uses: actions/upload-artifact@" not in body:
            continue
        days = re.search(r"^ +retention-days: (\d+)$", body, re.M)
        if days:
            retentions[match.group("name")] = int(days.group(1))
    return retentions


def smoke_step_retention_days(name: str) -> int:
    """The retention declared by a step inside the PR Smoke job."""
    metadata, _ = smoke_step(name)
    match = re.search(r"^ +retention-days: (\d+)$", metadata, re.M)
    assert match, f"workflow step declares no retention-days: {name}"
    return int(match.group(1))


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
        self.write_baseline(85, 5)
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
            "CARGO_TARGET_DIR": "target",
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

    def write_baseline(self, footprint_gib: int, reserve_gib: int) -> None:
        path = self.repo / BASELINE
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(
            '{\n  "schema_version": 1,\n'
            f'  "footprint_gib": {footprint_gib},\n'
            f'  "reserve_gib": {reserve_gib},\n'
            '  "source_run_id": 37195503421,\n'
            '  "provenance_status": "expired-unverifiable",\n'
            '  "recorded_date": "2026-11-08",\n'
            '  "runner": "ubuntu-24.04"\n}\n',
            encoding="utf-8",
        )

    def commit(self, message: str) -> None:
        self.git("add", "--", HELPER)
        if (self.repo / BASELINE).exists():
            self.git("add", "--", BASELINE)
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

    def test_committed_baseline_change_moves_the_hosted_floor(self) -> None:
        # The declared bound lives in the committed baseline at the
        # workflow SHA: raising it there (86 GiB footprint) must move
        # the preflight floor to 91 GiB with no workflow literal edit.
        self.git("checkout", "-q", "workflow")
        self.write_baseline(86, 5)
        self.commit("raise committed footprint baseline")
        self.env["PR_SMOKE_WORKFLOW_SHA"] = self.git("rev-parse", "HEAD").stdout.strip()
        self.git("checkout", "-q", "candidate")
        result = self.materialize()
        self.assertEqual(result.returncode, 0, result.stderr)
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
        metadata, body = smoke_step("Resolve workflow-bound PR Smoke disk guard")
        self.assertIn("PR_SMOKE_WORKFLOW_SHA: ${{ github.workflow_sha }}", metadata)
        self.assertIn(".ci/pr-smoke-footprint-baseline.json", body)
        self.assertIn("PR_SMOKE_BASELINE_PATH", body)
        _, preflight_body = smoke_step("Pre-flight disk budget guard")
        self.assertIn("--baseline-path", preflight_body)
        preflight_metadata, _ = smoke_step("Pre-flight disk budget guard")
        self.assertIn("id: pr-smoke-disk-preflight", preflight_metadata)
        metadata, _ = smoke_step("Classify PR-fast disk exhaustion")
        self.assertIn("steps.pr-smoke-disk-preflight.outcome == 'success'", metadata)
        self.assertIn("steps.pr-smoke-gate-runner.outcome != 'skipped'", metadata)
        receipt_metadata, _ = smoke_step("Upload PR-fast receipt")
        self.assertIn("hashFiles('target/receipts/receipt.json') != ''", receipt_metadata)
        missing_metadata, _ = smoke_step("Upload PR-fast missing-receipt diagnostics")
        self.assertIn("hashFiles('target/receipts/receipt.json') == ''", missing_metadata)
        self.assertIn("pr-fast-missing-receipt-", missing_metadata)


class ReceiptRetentionTests(unittest.TestCase):
    """The PR Smoke receipt must outlive the evidence it explains (#19067).

    The declared footprint bound may be revised only with a fresh measured
    receipt (PR_SMOKE_FOOTPRINT_GIB), and that measurement rides in
    pr-fast-disk-pressure.log inside the uploaded receipt. When the receipt
    expires before anyone can review it, the rule cannot be obeyed and the
    bound is an assertion nobody can check. The bound's own originating run
    (37195503421) has no surviving artifact and no job log.
    """

    PR_SMOKE_UPLOADS = (
        "Upload PR-fast receipt",
        "Upload PR-fast missing-receipt diagnostics",
    )
    PEER_UPLOADS = ("Upload gate receipt", "Upload CI actuals")

    def test_pr_smoke_receipt_outlives_the_shard_receipts(self) -> None:
        uploads = upload_retention_days()
        for peer_name in self.PEER_UPLOADS:
            self.assertIn(peer_name, uploads)
            for name in self.PR_SMOKE_UPLOADS:
                with self.subTest(upload=name, peer=peer_name):
                    self.assertIn(name, uploads)
                    self.assertGreaterEqual(
                        uploads[name],
                        uploads[peer_name],
                        f"{name} expires before {peer_name}",
                    )

    def test_pr_smoke_receipt_retained_at_least_thirty_days(self) -> None:
        for name in self.PR_SMOKE_UPLOADS:
            self.assertGreaterEqual(
                smoke_step_retention_days(name),
                30,
                f"{name} must survive long enough for a measured footprint "
                "receipt to be reviewed before the bound is revised",
            )

    def test_disk_pressure_log_rides_with_the_pr_smoke_receipt(self) -> None:
        for name in self.PR_SMOKE_UPLOADS:
            metadata, _ = smoke_step(name)
            self.assertIn(
                "target/receipts/logs/*.log",
                metadata,
                f"{name} does not upload the log directory that carries "
                "pr-fast-disk-pressure.log",
            )


if __name__ == "__main__":
    unittest.main()
