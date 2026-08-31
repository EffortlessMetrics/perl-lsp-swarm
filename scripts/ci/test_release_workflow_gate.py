#!/usr/bin/env python3
"""Tests for scripts/ci/release_workflow_gate.py."""

from __future__ import annotations

import datetime as dt
import importlib.util
import pathlib
import sys
import unittest

MODULE_PATH = pathlib.Path(__file__).with_name("release_workflow_gate.py")
SPEC = importlib.util.spec_from_file_location("release_workflow_gate", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {MODULE_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)

GateError = MODULE.GateError
RunIdentity = MODULE.RunIdentity
select_new_exact_run = MODULE.select_new_exact_run
validate_terminal_run = MODULE.validate_terminal_run

UTC = dt.timezone.utc
START = dt.datetime(2026, 8, 31, 12, 0, tzinfo=UTC)
SHA = "a" * 40
WORKFLOW_ID = 77


def run(
    run_id: int,
    *,
    sha: str = SHA,
    event: str = "workflow_dispatch",
    status: str = "completed",
    conclusion: str | None = "success",
    created_at: str = "2026-08-31T12:00:01Z",
    workflow_id: int = WORKFLOW_ID,
    attempt: int = 1,
) -> RunIdentity:
    return RunIdentity(
        run_id=run_id,
        run_attempt=attempt,
        workflow_id=workflow_id,
        event=event,
        head_sha=sha,
        head_branch="v0.18.0-rc.2",
        status=status,
        conclusion=conclusion,
        html_url=f"https://example.invalid/runs/{run_id}",
        created_at=created_at,
    )


class SelectionTests(unittest.TestCase):
    def test_selects_only_new_exact_sha_run(self) -> None:
        selected = select_new_exact_run(
            [
                run(1),
                run(2, sha="b" * 40),
                run(3, event="push"),
                run(4),
            ],
            prior_ids={1},
            expected_sha=SHA,
            dispatch_started=START,
        )
        self.assertIsNotNone(selected)
        self.assertEqual(selected.run_id, 4)

    def test_returns_none_until_exact_run_appears(self) -> None:
        selected = select_new_exact_run(
            [run(1), run(2, sha="b" * 40)],
            prior_ids={1},
            expected_sha=SHA,
            dispatch_started=START,
        )
        self.assertIsNone(selected)

    def test_rejects_ambiguous_new_runs(self) -> None:
        with self.assertRaisesRegex(GateError, "multiple new"):
            select_new_exact_run(
                [run(2), run(3)],
                prior_ids=set(),
                expected_sha=SHA,
                dispatch_started=START,
            )

    def test_rejects_old_run_even_when_id_is_new_to_observer(self) -> None:
        selected = select_new_exact_run(
            [run(2, created_at="2026-08-31T11:58:00Z")],
            prior_ids=set(),
            expected_sha=SHA,
            dispatch_started=START,
        )
        self.assertIsNone(selected)


class TerminalValidationTests(unittest.TestCase):
    def test_accepts_exact_success(self) -> None:
        validate_terminal_run(
            run(5), expected_sha=SHA, expected_workflow_id=WORKFLOW_ID
        )

    def test_rejects_wrong_workflow(self) -> None:
        with self.assertRaisesRegex(GateError, "workflow mismatch"):
            validate_terminal_run(
                run(5, workflow_id=88),
                expected_sha=SHA,
                expected_workflow_id=WORKFLOW_ID,
            )

    def test_rejects_wrong_sha(self) -> None:
        with self.assertRaisesRegex(GateError, "source mismatch"):
            validate_terminal_run(
                run(5, sha="b" * 40),
                expected_sha=SHA,
                expected_workflow_id=WORKFLOW_ID,
            )

    def test_rejects_non_terminal(self) -> None:
        with self.assertRaisesRegex(GateError, "not terminal"):
            validate_terminal_run(
                run(5, status="in_progress", conclusion=None),
                expected_sha=SHA,
                expected_workflow_id=WORKFLOW_ID,
            )

    def test_rejects_every_non_success_conclusion(self) -> None:
        for conclusion in sorted(MODULE.TERMINAL_NON_SUCCESS):
            with self.subTest(conclusion=conclusion):
                with self.assertRaisesRegex(GateError, "did not succeed"):
                    validate_terminal_run(
                        run(5, conclusion=conclusion),
                        expected_sha=SHA,
                        expected_workflow_id=WORKFLOW_ID,
                    )

    def test_rejects_missing_conclusion(self) -> None:
        with self.assertRaisesRegex(GateError, "conclusion=missing"):
            validate_terminal_run(
                run(5, conclusion=None),
                expected_sha=SHA,
                expected_workflow_id=WORKFLOW_ID,
            )


if __name__ == "__main__":
    unittest.main()
