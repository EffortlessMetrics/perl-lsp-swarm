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
        main_workflow_sha: str = "ci-workflow-sha",
        candidate_workflow_sha: str = "ci-workflow-sha",
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
            main_workflow_sha=main_workflow_sha,
            candidate_workflow_sha=candidate_workflow_sha,
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

    def test_base_only_workflow_advancement_keeps_exact_subject_evidence_comparable(self) -> None:
        """Main advanced ci.yml; PR head still has the old blob; merge tree matches main.

        Check runs stay bound to the PR head subject. Comparability must use
        the effective merge/event workflow blob, not the stale head blob.
        """
        merge_tree = refusal.effective_workflow_tree_sha(
            event_name="pull_request",
            github_sha="merge-tree-sha",
            pr_head_sha="pr-head-sha",
        )
        blob_by_tree = {
            "merge-tree-sha": "new-main-ci-blob",
            "pr-head-sha": "stale-pr-head-ci-blob",
        }
        decision = self.evaluate(
            main=all_shards("main-sha", conclusion="failure"),
            candidate=all_shards("pr-head-sha", conclusion="success"),
            subject="pr-head-sha",
            main_workflow_sha="new-main-ci-blob",
            candidate_workflow_sha=blob_by_tree[merge_tree],
        )
        self.assertFalse(decision.blocks)
        self.assertFalse(decision.waits_for_candidate)
        self.assertNotIn("not comparable", " ".join(decision.warnings))

    def test_reading_stale_pr_head_workflow_blob_is_the_false_positive_to_avoid(self) -> None:
        """Wrong implementation: compare the PR-head ci.yml after a base-only advancement."""
        decision = self.evaluate(
            main=all_shards("main-sha", conclusion="failure"),
            candidate=all_shards("pr-head-sha", conclusion="success"),
            subject="pr-head-sha",
            main_workflow_sha="new-main-ci-blob",
            candidate_workflow_sha="stale-pr-head-ci-blob",
        )
        self.assertTrue(decision.waits_for_candidate)
        self.assertIn("not comparable", " ".join(decision.warnings))
        self.assertTrue(refusal.finalize(decision).blocks)

    def test_pr_authored_ci_yml_change_is_not_comparable(self) -> None:
        decision = self.evaluate(
            main=all_shards("main-sha", conclusion="failure"),
            candidate=all_shards("candidate-sha", conclusion="success"),
            main_workflow_sha="main-workflow-sha",
            candidate_workflow_sha="pr-authored-workflow-sha",
        )
        self.assertTrue(decision.waits_for_candidate)
        self.assertIn("differs from canonical main", " ".join(decision.warnings))
        self.assertTrue(refusal.finalize(decision).blocks)

    def test_missing_effective_workflow_tree_is_not_comparable(self) -> None:
        decision = self.evaluate(
            main=all_shards("main-sha", conclusion="failure"),
            candidate=all_shards("candidate-sha", conclusion="success"),
            main_workflow_sha="main-workflow-sha",
            candidate_workflow_sha="",
        )
        self.assertTrue(decision.waits_for_candidate)
        self.assertIn("could not be read", " ".join(decision.warnings))
        self.assertTrue(refusal.finalize(decision).blocks)

    def test_missing_exact_subject_run_evidence_waits_then_fails_closed(self) -> None:
        decision = self.evaluate(
            main=all_shards("main-sha", conclusion="failure"),
            candidate=all_shards("candidate-sha", conclusion="success"),
            candidate_workflow_run_ids=set(),
        )
        self.assertTrue(decision.waits_for_candidate)
        self.assertFalse(decision.blocks)
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


class EffectiveWorkflowTreeTests(unittest.TestCase):
    def test_pull_request_uses_github_sha_never_pr_head(self) -> None:
        self.assertEqual(
            refusal.effective_workflow_tree_sha(
                event_name="pull_request",
                github_sha="merge-tree-sha",
                pr_head_sha="pr-head-sha",
            ),
            "merge-tree-sha",
        )

    def test_merge_group_uses_merge_group_head(self) -> None:
        self.assertEqual(
            refusal.effective_workflow_tree_sha(
                event_name="merge_group",
                github_sha="github-sha-should-not-win",
                merge_group_head_sha="merge-group-head",
                pr_head_sha="pr-head-sha",
            ),
            "merge-group-head",
        )

    def test_missing_effective_tree_does_not_fall_back_to_pr_head(self) -> None:
        self.assertEqual(
            refusal.effective_workflow_tree_sha(
                event_name="pull_request",
                github_sha="",
                pr_head_sha="pr-head-sha",
            ),
            "",
        )
        self.assertEqual(
            refusal.effective_workflow_tree_sha(
                event_name="merge_group",
                github_sha="github-sha-should-not-win",
                merge_group_head_sha="",
                pr_head_sha="pr-head-sha",
            ),
            "",
        )

    def test_non_candidate_events_have_no_effective_tree(self) -> None:
        self.assertEqual(
            refusal.effective_workflow_tree_sha(
                event_name="push",
                github_sha="main-sha",
                pr_head_sha="pr-head-sha",
            ),
            "",
        )


class MainRedRefusalWorkflowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = WORKFLOW.read_text(encoding="utf-8")
        self.probe = (ROOT / ".ci/rust-main-red-probe.sh").read_text()

    def test_probe_keeps_read_scopes_and_landed_baseline(self):
        self.assertIn("  checks: read", self.workflow)
        self.assertIn("  push:\n    branches: [main, master]", self.workflow)
        self.assertIn("result_script: .ci/rust-standard-result.sh", self.workflow)
        self.assertNotIn("runs-on:", self.workflow)

    def test_probe_reads_exact_main_candidate_and_effective_tree(self):
        probe = self.probe
        self.assertEqual(probe.count("$(read_main_sha)"), 3)
        for fragment in ("git/ref/heads/main", "commits/${MAIN_SHA_BEFORE}/check-runs",
                         "commits/${CANDIDATE_SHA}/check-runs",
                         "actions/workflows/ci.yml/runs?head_sha=${MAIN_SHA_BEFORE}",
                         "actions/workflows/ci.yml/runs?head_sha=${CANDIDATE_SHA}",
                         "contents/.github/workflows/ci.yml?ref=$1",
                         "contents/scripts/ci/main_red_refusal.py?ref=${MAIN_SHA_BEFORE}",
                         'python3 "$TRUSTED_SCRIPT"', "TRUSTED_SCRIPT_AVAILABLE",
                         "--main-workflow-sha", "--candidate-workflow-sha"):
            self.assertIn(fragment, probe)
        self.assertEqual(probe.count('read_workflow_sha "$EFFECTIVE_WORKFLOW_TREE"'), 2)
        self.assertNotIn('read_workflow_sha "$CANDIDATE_SHA"', probe)

    def test_refusal_and_bounded_wait_are_preserved(self):
        self.assertIn('if [ "$refusal_status" -eq 1 ]; then\n  exit 1', self.probe)
        self.assertIn('"$poll_attempt" -lt 50', self.probe)
        self.assertIn('sleep 30', self.probe)
        self.assertIn('run_refusal --final', self.probe)
        self.assertIn('main-red refusal probe is non-applicable', self.probe)


if __name__ == "__main__":
    unittest.main()
