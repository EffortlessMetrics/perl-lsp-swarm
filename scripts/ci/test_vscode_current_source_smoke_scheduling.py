#!/usr/bin/env python3
"""Falsifiers for the VS Code current-source smoke scheduling observer (#6887)."""

from __future__ import annotations

import importlib.util
import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "scripts/ci/fixtures/vscode_current_source_smoke_scheduling"

SCRIPT = Path(__file__).with_name("vscode_current_source_smoke_scheduling.py")
SPEC = importlib.util.spec_from_file_location("vscode_current_source_smoke_scheduling", SCRIPT)
assert SPEC and SPEC.loader
scheduling = importlib.util.module_from_spec(SPEC)
sys.modules["vscode_current_source_smoke_scheduling"] = scheduling
SPEC.loader.exec_module(scheduling)


def smoke_run(
    run_id: int,
    *,
    head_sha: str,
    status: str,
    conclusion: str | None = None,
    job_count: int | None = None,
    pull_number: int = 4242,
) -> dict:
    payload: dict = {
        "id": run_id,
        "name": "VS Code Current-Source Linux Smoke",
        "path": scheduling.WORKFLOW_PATH,
        "head_sha": head_sha,
        "status": status,
        "pull_requests": [pull_number],
    }
    if conclusion is not None:
        payload["conclusion"] = conclusion
    if job_count is not None:
        payload["job_count"] = job_count
    return payload


class ClassificationTests(unittest.TestCase):
    def test_active_run_is_not_killed_by_later_push_sequence(self) -> None:
        head_a = "a" * 40
        head_b = "b" * 40
        head_c = "c" * 40
        runs = [
            smoke_run(100, head_sha=head_a, status="completed", conclusion="success", job_count=2),
            smoke_run(101, head_sha=head_b, status="completed", conclusion="cancelled", job_count=0),
            smoke_run(102, head_sha=head_c, status="queued", job_count=0),
        ]
        receipt = scheduling.observe_pull_request_snapshot(
            runs, pull_request_number=4242, latest_head_sha=head_c, observed_at="2026-11-02T00:00:00+00:00"
        )
        by_id = {entry["run_id"]: entry for entry in receipt["runs"]}
        self.assertEqual(by_id[100]["terminal_state"], scheduling.COMPLETED_EXACT_SUBJECT)
        self.assertEqual(by_id[101]["terminal_state"], scheduling.SUPERSEDED_WHILE_QUEUED)
        self.assertEqual(by_id[102]["terminal_state"], scheduling.QUEUED_LATEST_CANDIDATE)
        self.assertEqual(receipt["measurement"]["superseded_while_queued_count"], 1)
        self.assertTrue(
            receipt["measurement"]["active_run_reached_terminal_while_newer_heads_arrived"]
        )

    def test_completed_older_head_cannot_authorize_latest_merge(self) -> None:
        head_a = "a" * 40
        head_c = "c" * 40
        runs = [
            smoke_run(100, head_sha=head_a, status="completed", conclusion="success", job_count=2),
        ]
        receipt = scheduling.observe_pull_request_snapshot(
            runs, pull_request_number=4242, latest_head_sha=head_c, observed_at="2026-11-02T00:00:00+00:00"
        )
        entry = receipt["runs"][0]
        self.assertEqual(entry["terminal_state"], scheduling.COMPLETED_EXACT_SUBJECT)
        self.assertEqual(entry["latest_head_relationship"], "exact_subject_not_latest_head")
        self.assertEqual(receipt["merge_verdict_authority"], scheduling.MERGE_VERDICT_AUTHORITY)

    def test_platform_cancellation_after_start_is_distinct_from_supersession(self) -> None:
        head_a = "a" * 40
        runs = [
            smoke_run(
                100,
                head_sha=head_a,
                status="completed",
                conclusion="cancelled",
                job_count=2,
            ),
        ]
        receipt = scheduling.observe_pull_request_snapshot(
            runs, pull_request_number=4242, latest_head_sha=head_a, observed_at="2026-11-02T00:00:00+00:00"
        )
        self.assertEqual(
            receipt["runs"][0]["terminal_state"],
            scheduling.CANCELLED_BY_HUMAN_OR_PLATFORM,
        )

    def test_product_failure_is_distinct_from_supersession(self) -> None:
        head_c = "c" * 40
        runs = [
            smoke_run(100, head_sha=head_c, status="completed", conclusion="failure", job_count=2),
        ]
        receipt = scheduling.observe_pull_request_snapshot(
            runs, pull_request_number=4242, latest_head_sha=head_c, observed_at="2026-11-02T00:00:00+00:00"
        )
        self.assertEqual(receipt["runs"][0]["terminal_state"], scheduling.FAILED)


class FixtureTests(unittest.TestCase):
    def test_checked_multi_push_fixture_matches_classifier(self) -> None:
        fixture_path = FIXTURES / "multi_push_observed.v1.json"
        self.assertTrue(fixture_path.is_file(), "missing observed multi-push fixture")
        fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
        scheduling.validate_observation_receipt(fixture)
        source_runs = fixture["source_runs"]
        rebuilt = scheduling.observe_pull_request_snapshot(
            source_runs,
            pull_request_number=fixture["pull_request"],
            latest_head_sha=fixture["latest_observed_head_sha"],
            observed_at=fixture["observed_at"],
        )
        self.assertEqual(rebuilt["runs"], fixture["runs"])
        self.assertEqual(rebuilt["measurement"], fixture["measurement"])


if __name__ == "__main__":
    unittest.main()
