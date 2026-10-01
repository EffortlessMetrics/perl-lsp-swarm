#!/usr/bin/env python3
"""Tests for scripts/ci/release_workflow_gate.py."""

from __future__ import annotations

from dataclasses import replace
from unittest.mock import patch
import json
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
REPO = "EffortlessMetrics/perl-lsp-swarm"
REF = "refs/tags/v0.18.0-rc.2"
TRANSACTION = "c" * 64
WORKFLOW_REF = REPO + "/.github/workflows/release.yml@" + REF


def observation(**changes):
    value = {"schema_version": "private_producer_observation.v1", "phase": "private_candidate",
             "qualification": "not_proven", "transaction_id": TRANSACTION, "source_sha": SHA,
             "producer": {"repository": REPO, "workflow_ref": WORKFLOW_REF, "run_id": 5, "run_attempt": 1}}
    value.update(changes)
    return json.dumps(value).encode()


def select_new_exact_run(runs, **kwargs):
    return MODULE.select_new_exact_run(runs, expected_repository=REPO, expected_ref=REF,
        expected_workflow_id=WORKFLOW_ID, expected_run_attempt=1, **kwargs)


def validate_terminal_run(run, **kwargs):
    return MODULE.validate_terminal_run(run, expected_repository=REPO, expected_ref=REF,
        expected_run_id=5, expected_run_attempt=1, expected_transaction_id=TRANSACTION,
        expected_workflow_ref=WORKFLOW_REF, observation_bytes=observation(), **kwargs)

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
        repository=REPO,
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

    def test_rejects_terminal_event_mismatch(self) -> None:
        with self.assertRaisesRegex(GateError, "event mismatch"):
            validate_terminal_run(run(5, event="push"), expected_sha=SHA, expected_workflow_id=WORKFLOW_ID)

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


class ExactIdentityControls(unittest.TestCase):
    def test_selection_ref_workflow_attempt_and_selected_id(self):
        for changed in [replace(run(5), head_branch="wrong"), run(5, workflow_id=88), run(5, attempt=2), replace(run(5), repository="other/repo")]:
            selected = select_new_exact_run([changed], prior_ids=set(), expected_sha=SHA, dispatch_started=START)
            if selected is not None:
                raise RuntimeError("wrong same-SHA run selected")
        selected = select_new_exact_run([run(6)], prior_ids=set(), expected_sha=SHA, dispatch_started=START, expected_run_id=5)
        if selected is not None:
            raise RuntimeError("different selected run ID accepted")

    def test_terminal_rejects_ref_run_and_attempt_substitution(self):
        for changed in [replace(run(5), head_branch="wrong"), run(6), run(5, attempt=2)]:
            with self.assertRaises(GateError):
                validate_terminal_run(changed, expected_sha=SHA, expected_workflow_id=WORKFLOW_ID)

    def test_same_sha_distinct_transaction_and_artifact_identity_refuse(self):
        arguments = dict(expected_repository=REPO, expected_ref=REF, expected_run_id=5,
            expected_run_attempt=1, expected_transaction_id=TRANSACTION, expected_workflow_ref=WORKFLOW_REF,
            expected_sha=SHA, expected_workflow_id=WORKFLOW_ID)
        for raw in [None, observation(transaction_id="d" * 64), observation(source_sha="b" * 40),
                    observation(producer={"repository": REPO, "workflow_ref": WORKFLOW_REF, "run_id": 6, "run_attempt": 1}),
                    observation(producer={"repository": REPO, "workflow_ref": WORKFLOW_REF, "run_id": 5, "run_attempt": 2}),
                    observation(producer={"repository": REPO, "workflow_ref": REPO + "/.github/workflows/other.yml@" + REF, "run_id": 5, "run_attempt": 1})]:
            with self.assertRaises(GateError):
                MODULE.validate_terminal_run(run(5), observation_bytes=raw, **arguments)

    def test_missing_artifact_adapter_never_dispatches(self):
        with patch.object(MODULE, "_dispatch", side_effect=RuntimeError("unexpected dispatch")) as dispatch:
            with self.assertRaisesRegex(GateError, "NOT_PROVEN"):
                MODULE.dispatch_and_wait(repo=REPO, workflow="release.yml", ref=REF,
                    expected_sha=SHA, fields=[], timeout_seconds=1, poll_seconds=1)
            if dispatch.call_count:
                raise RuntimeError("missing identity adapter dispatched")

    def test_mocked_exact_api_and_named_private_artifact(self):
        calls = []
        def loader(repo, run_id, name):
            calls.append((repo, run_id, name))
            return {"repository": repo, "run_id": run_id, "name": name, "bytes": observation()}
        # Entire platform and dispatch are mocked; no network or publication runs.
        with patch.object(MODULE, "_workflow", return_value={"id": WORKFLOW_ID, "path": ".github/workflows/release.yml"}), patch.object(MODULE, "_runs", side_effect=[[], [run(5, created_at="2999-01-01T00:00:00Z")]]), patch.object(MODULE, "_run", return_value=run(5)), patch.object(MODULE, "_dispatch"):
            result = MODULE.dispatch_and_wait(repo=REPO, workflow="release.yml", ref=REF,
                expected_sha=SHA, fields=[], timeout_seconds=10, poll_seconds=1,
                expected_transaction_id=TRANSACTION, expected_workflow_ref=WORKFLOW_REF,
                observation_loader=loader)
        if result.run_id != 5 or calls != [(REPO, 5, "private-producer-" + TRANSACTION + "-1")]:
            raise RuntimeError("named artifact/run join changed")


if __name__ == "__main__":
    unittest.main()
