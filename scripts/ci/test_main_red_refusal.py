#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any

MODULE_PATH = Path(__file__).with_name("main_red_refusal.py")
SPEC = importlib.util.spec_from_file_location("main_red_refusal", MODULE_PATH)
assert SPEC and SPEC.loader
refusal = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = refusal
SPEC.loader.exec_module(refusal)

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "em-ci-routed-rust.yml"


def run(name: str, sha: str, status: str = "completed", conclusion: str | None = "success", run_id: int = 1) -> dict[str, Any]:
    return {
        "id": run_id,
        "name": name,
        "head_sha": sha,
        "details_url": f"https://github.com/example/actions/runs/{run_id}/job/{run_id}",
        "status": status,
        "conclusion": conclusion,
        "started_at": f"2026-08-25T19:00:{run_id:02d}Z",
        "app": {"id": refusal.GITHUB_ACTIONS_APP_ID},
    }


def all_shards(sha: str, *, conclusion: str = "success", status: str = "completed") -> list[dict[str, Any]]:
    return [run(name, sha, status, conclusion, index) for index, name in enumerate(refusal.SHARD_NAMES, 1)]


class MainRedRefusalBehaviorTests(unittest.TestCase):
    def evaluate(
        self,
        *,
        main: list[dict[str, Any]],
        candidate: list[dict[str, Any]],
        before: str = "main-sha",
        after: str = "main-sha",
        subject: str = "candidate-sha",
        main_workflow_run_ids: set[int] | None = None,
        candidate_workflow_run_ids: set[int] | None = None,
    ) -> refusal.Decision:
        return refusal.evaluate(
            main_runs=main,
            candidate_runs=candidate,
            main_sha_before=before,
            main_sha_after=after,
            candidate_sha=subject,
            main_workflow_run_ids=(
                set(range(1, len(refusal.SHARD_NAMES) + 1))
                if main_workflow_run_ids is None
                else main_workflow_run_ids
            ),
            candidate_workflow_run_ids=(
                set(range(1, len(refusal.SHARD_NAMES) + 1))
                if candidate_workflow_run_ids is None
                else candidate_workflow_run_ids
            ),
            main_workflow_sha="ci-workflow-sha",
            candidate_workflow_sha="ci-workflow-sha",
        )

    def test_red_main_and_red_candidate_blocks(self) -> None:
        decision = self.evaluate(
            main=all_shards("main-sha", conclusion="failure"),
            candidate=all_shards("candidate-sha", conclusion="failure"),
            main_workflow_run_ids=set(range(1, 9)),
            candidate_workflow_run_ids=set(range(1, 9)),
        )
        self.assertTrue(decision.blocks)
        self.assertIn("CI Gate shard (meta)", decision.blockers[0])

    def test_green_main_never_blocks_a_red_candidate(self) -> None:
        decision = self.evaluate(
            main=all_shards("main-sha", conclusion="success"),
            candidate=all_shards("candidate-sha", conclusion="failure"),
            main_workflow_run_ids=set(range(1, 9)),
            candidate_workflow_run_ids=set(range(1, 9)),
        )
        self.assertFalse(decision.blocks)

    def test_cancelled_main_is_not_a_recorded_red(self) -> None:
        decision = self.evaluate(
            main=all_shards("main-sha", conclusion="cancelled"),
            candidate=all_shards("candidate-sha", conclusion="failure"),
        )
        self.assertFalse(decision.blocks)

    def test_red_main_and_green_candidate_are_non_blocking(self) -> None:
        decision = self.evaluate(
            main=all_shards("main-sha", conclusion="failure"),
            candidate=all_shards("candidate-sha", conclusion="success"),
            main_workflow_run_ids=set(range(1, 9)),
            candidate_workflow_run_ids=set(range(1, 9)),
        )
        self.assertFalse(decision.blocks)

    def test_main_red_without_candidate_evidence_waits_then_blocks(self) -> None:
        decision = self.evaluate(main=all_shards("main-sha", conclusion="failure"), candidate=[])
        self.assertTrue(decision.waits_for_candidate)
        final = refusal.finalize(decision)
        self.assertTrue(final.blocks)
        self.assertIn("no exact-SHA", final.blockers[0])

    def test_nonterminal_candidate_does_not_repair_main_red(self) -> None:
        candidate = all_shards("candidate-sha", conclusion="success")
        candidate[0] = run(refusal.SHARD_NAMES[0], "candidate-sha", "queued", None)
        decision = self.evaluate(
            main=all_shards("main-sha", conclusion="failure"),
            candidate=candidate,
        )
        self.assertTrue(decision.waits_for_candidate)
        self.assertFalse(decision.blocks)

    def test_cancelled_and_skipped_candidate_require_retry(self) -> None:
        for status, conclusion in (
            ("completed", "cancelled"),
            ("completed", "skipped"),
        ):
            candidate = all_shards("candidate-sha", conclusion="success")
            candidate[0] = run(refusal.SHARD_NAMES[0], "candidate-sha", status, conclusion)
            decision = self.evaluate(
                main=all_shards("main-sha", conclusion="failure"),
                candidate=candidate,
            )
            self.assertTrue(decision.waits_for_candidate, (status, conclusion))
            self.assertTrue(refusal.finalize(decision).blocks)

    def test_neutral_candidate_does_not_repair_main_red(self) -> None:
        candidate = all_shards("candidate-sha", conclusion="success")
        candidate[0] = run(refusal.SHARD_NAMES[0], "candidate-sha", conclusion="neutral")
        decision = self.evaluate(
            main=all_shards("main-sha", conclusion="failure"),
            candidate=candidate,
        )
        self.assertTrue(decision.blocks)

    def test_main_movement_after_lookup_makes_red_evidence_stale(self) -> None:
        decision = self.evaluate(
            main=all_shards("main-sha", conclusion="failure"),
            candidate=all_shards("candidate-sha", conclusion="failure"),
            after="new-main-sha",
        )
        self.assertFalse(decision.blocks)
        self.assertIn("stale", " ".join(decision.warnings))

    def test_incomplete_probe_data_cannot_block(self) -> None:
        decision = refusal.evaluate(
            main_runs=all_shards("main-sha", conclusion="failure"),
            candidate_runs=all_shards("candidate-sha", conclusion="failure"),
            main_sha_before="main-sha",
            main_sha_after="main-sha",
            candidate_sha="candidate-sha",
            candidate_probe_warning="HTTP 500",
            main_workflow_run_ids=set(range(1, 9)),
            candidate_workflow_run_ids=set(range(1, 9)),
            main_workflow_sha="ci-workflow-sha",
            candidate_workflow_sha="ci-workflow-sha",
        )
        self.assertTrue(decision.blocks)
        self.assertIn("HTTP 500", " ".join(decision.warnings))

    def test_incomplete_main_probe_data_is_non_blocking(self) -> None:
        decision = refusal.evaluate(
            main_runs=all_shards("main-sha", conclusion="failure"),
            candidate_runs=all_shards("candidate-sha", conclusion="failure"),
            main_sha_before="main-sha",
            main_sha_after="main-sha",
            candidate_sha="candidate-sha",
            main_workflow_probe_warning="HTTP 500",
            main_workflow_run_ids=set(),
            candidate_workflow_run_ids=set(range(1, 9)),
            main_workflow_sha="ci-workflow-sha",
            candidate_workflow_sha="ci-workflow-sha",
        )
        self.assertFalse(decision.blocks)
        self.assertIn("non-blocking", " ".join(decision.warnings))

    def test_old_sha_runs_are_not_comparable(self) -> None:
        decision = self.evaluate(
            main=all_shards("old-main-sha", conclusion="failure"),
            candidate=all_shards("old-candidate-sha", conclusion="failure"),
        )
        self.assertFalse(decision.blocks)
        self.assertEqual(len(decision.warnings), len(refusal.SHARD_NAMES))

    def test_newer_in_progress_rerun_wins_over_old_completed_run(self) -> None:
        main = all_shards("main-sha", conclusion="failure")
        candidate = all_shards("candidate-sha", conclusion="success")
        candidate.extend(
            run(refusal.SHARD_NAMES[0], "candidate-sha", "in_progress", None, 100)
            for _ in range(1)
        )
        decision = self.evaluate(
            main=main,
            candidate=candidate,
            candidate_workflow_run_ids=set(range(1, 9)) | {100},
        )
        self.assertTrue(decision.waits_for_candidate)
        self.assertFalse(decision.blocks)

    def test_neutral_and_unknown_candidate_results_do_not_repair_main_red(self) -> None:
        for conclusion in ("neutral", "stale", "action_required"):
            candidate = all_shards("candidate-sha", conclusion="success")
            candidate[0] = run(refusal.SHARD_NAMES[0], "candidate-sha", conclusion=conclusion)
            decision = self.evaluate(
                main=all_shards("main-sha", conclusion="failure"),
                candidate=candidate,
            )
            self.assertTrue(decision.blocks, conclusion)

    def test_noncanonical_same_name_run_is_not_comparable(self) -> None:
        main = all_shards("main-sha", conclusion="failure")
        candidate = all_shards("candidate-sha", conclusion="success")
        candidate[0]["details_url"] = "https://github.com/example/actions/runs/999/job/999"
        decision = self.evaluate(
            main=main,
            candidate=candidate,
            candidate_workflow_run_ids=set(range(1, 9)),
        )
        self.assertTrue(decision.waits_for_candidate)
        self.assertFalse(decision.blocks)
        self.assertTrue(refusal.finalize(decision).blocks)

    def test_changed_candidate_workflow_is_not_comparable(self) -> None:
        decision = refusal.evaluate(
            main_runs=all_shards("main-sha", conclusion="failure"),
            candidate_runs=all_shards("candidate-sha", conclusion="success"),
            main_sha_before="main-sha",
            main_sha_after="main-sha",
            candidate_sha="candidate-sha",
            main_workflow_run_ids=set(range(1, 9)),
            candidate_workflow_run_ids=set(range(1, 9)),
            main_workflow_sha="main-workflow-sha",
            candidate_workflow_sha="changed-workflow-sha",
        )
        self.assertTrue(decision.waits_for_candidate)
        self.assertTrue(refusal.finalize(decision).blocks)

    def test_payload_loader_accepts_slurped_pages(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "runs.json"
            path.write_text(
                json.dumps([{"check_runs": all_shards("main-sha")}]),
                encoding="utf-8",
            )
            runs, warning = refusal.load_payload(path)
        self.assertIsNone(warning)
        self.assertEqual(len(runs), len(refusal.SHARD_NAMES))

    def test_workflow_run_loader_requires_exact_subject(self) -> None:
        runs, warning = refusal.load_workflow_run_ids(
            Path("missing-workflow-runs.json"),
            "main-sha",
        )
        self.assertIsNotNone(warning)
        self.assertEqual(set(), runs)


class MainRedRefusalWorkflowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = WORKFLOW.read_text(encoding="utf-8")

    def test_probe_has_read_permission_and_no_push_trigger(self) -> None:
        self.assertIn("  checks: read", self.workflow)
        self.assertNotIn("\n  push:", self.workflow)
        self.assertNotIn("pr-smoke", self.workflow)
        probe_start = self.workflow.index("      - name: Probe main-red refusal")
        evaluate_start = self.workflow.index("      - name: Evaluate routed result")
        probe = self.workflow[probe_start:evaluate_start]
        self.assertIn(
            "(github.event_name != 'pull_request' || github.event.pull_request.draft != true)",
            probe,
        )

    def test_probe_reads_main_before_and_after_exact_check_lookup(self) -> None:
        probe_start = self.workflow.index("      - name: Probe main-red refusal")
        evaluate_start = self.workflow.index("      - name: Evaluate routed result")
        probe = self.workflow[probe_start:evaluate_start]
        self.assertIn("git/ref/heads/main", probe)
        self.assertEqual(probe.count("$(read_main_sha)"), 3)
        self.assertIn("commits/${MAIN_SHA_BEFORE}/check-runs", probe)
        self.assertIn("commits/${CANDIDATE_SHA}/check-runs", probe)
        self.assertIn("actions/workflows/ci.yml/runs?head_sha=${MAIN_SHA_BEFORE}", probe)
        self.assertIn("actions/workflows/ci.yml/runs?head_sha=${CANDIDATE_SHA}", probe)
        self.assertIn("contents/.github/workflows/ci.yml?ref=$1", probe)
        self.assertIn("--main-workflow-sha", probe)
        self.assertIn("--candidate-workflow-sha", probe)
        self.assertIn("contents/scripts/ci/main_red_refusal.py?ref=${MAIN_SHA_BEFORE}", probe)
        self.assertIn('python3 "$TRUSTED_SCRIPT"', probe)
        self.assertIn("TRUSTED_SCRIPT_AVAILABLE", probe)
        self.assertIn("MAIN_SHA_AFTER", probe)

    def test_final_refusal_is_propagated_to_required_lane(self) -> None:
        probe_start = self.workflow.index("      - name: Probe main-red refusal")
        evaluate_start = self.workflow.index("      - name: Evaluate routed result")
        probe = self.workflow[probe_start:evaluate_start]
        self.assertIn('if [ "$refusal_status" -eq 1 ]; then', probe)
        self.assertIn("exit 1", probe)
        self.assertNotIn("continue-on-error: false", probe)


if __name__ == "__main__":
    unittest.main()
