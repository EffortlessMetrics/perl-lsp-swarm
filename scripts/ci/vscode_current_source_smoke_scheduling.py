#!/usr/bin/env python3
"""External observation for VS Code current-source Linux smoke scheduling (#6887).

The workflow emits an in-job ``vscode_current_source_scheduling.v1`` receipt for
runs that actually start. Queued heads that GitHub coalesces away never enter a
job, so their ``superseded_while_queued`` and ``queued_latest_candidate`` states
require a read-only observer over workflow-run snapshots.

This module is a pure classifier over those snapshots. It does not mutate branch
state, does not author merge readiness (#2911 remains the latest-head verdict
owner), and refuses to treat a superseded queued head as product failure.
"""

from __future__ import annotations

import datetime
import json
from typing import Any, Mapping, Sequence

WORKFLOW_PATH = ".github/workflows/vscode-current-source-linux-smoke.yml"

COMPLETED_EXACT_SUBJECT = "completed_exact_subject"
QUEUED_LATEST_CANDIDATE = "queued_latest_candidate"
SUPERSEDED_WHILE_QUEUED = "superseded_while_queued"
CANCELLED_BY_HUMAN_OR_PLATFORM = "cancelled_by_human_or_platform"
FAILED = "failed"
NOT_PROVEN = "not_proven"

TERMINAL_STATES = frozenset(
    {
        COMPLETED_EXACT_SUBJECT,
        QUEUED_LATEST_CANDIDATE,
        SUPERSEDED_WHILE_QUEUED,
        CANCELLED_BY_HUMAN_OR_PLATFORM,
        FAILED,
        NOT_PROVEN,
    }
)

OBSERVATION_SCHEMA = "vscode_current_source_smoke_scheduling_observation.v1"
OBSERVATION_KIND = "vscode_current_source_smoke_scheduling_observation"
MERGE_VERDICT_AUTHORITY = "#2911"


def concurrency_group_for_pull(pull_number: int) -> str:
    return f"vscode-current-source-linux-smoke-pr-{pull_number}"


def is_smoke_workflow_run(run: Mapping[str, Any]) -> bool:
    path = run.get("path")
    if isinstance(path, str) and path.endswith(WORKFLOW_PATH):
        return True
    name = run.get("name")
    return isinstance(name, str) and "Current-Source Linux Smoke" in name


def pull_number(run: Mapping[str, Any]) -> int | None:
    pulls = run.get("pull_requests")
    if isinstance(pulls, list) and pulls:
        first = pulls[0]
        if isinstance(first, int):
            return first
        if isinstance(first, Mapping):
            number = first.get("number")
            if isinstance(number, int):
                return number
    number = run.get("pull_number")
    return number if isinstance(number, int) else None


def job_count(run: Mapping[str, Any]) -> int | None:
    value = run.get("job_count")
    if isinstance(value, int) and value >= 0:
        return value
    return None


def _run_id(run: Mapping[str, Any]) -> int | None:
    run_id = run.get("id")
    return run_id if isinstance(run_id, int) else None


def newer_run_exists(*, run: Mapping[str, Any], runs: Sequence[Mapping[str, Any]]) -> bool:
    current_id = _run_id(run)
    if current_id is None:
        return False
    pull = pull_number(run)
    for candidate in runs:
        candidate_id = _run_id(candidate)
        if candidate_id is None or candidate_id <= current_id:
            continue
        if pull_number(candidate) != pull:
            continue
        if not is_smoke_workflow_run(candidate):
            continue
        return True
    return False


def classify_terminal_state(
    run: Mapping[str, Any],
    *,
    runs: Sequence[Mapping[str, Any]],
    latest_head_sha: str,
) -> str:
    """Map one workflow run to the #6887 terminal-state vocabulary."""
    status = run.get("status")
    conclusion = run.get("conclusion")
    head_sha = run.get("head_sha")
    jobs = job_count(run)

    if status in {"queued", "pending", "waiting", "requested"}:
        if isinstance(head_sha, str) and head_sha == latest_head_sha:
            return QUEUED_LATEST_CANDIDATE
        return NOT_PROVEN

    if status in {"in_progress"}:
        return NOT_PROVEN

    if status != "completed":
        return NOT_PROVEN

    if conclusion == "success":
        if jobs is not None and jobs > 0:
            return COMPLETED_EXACT_SUBJECT
        return NOT_PROVEN

    if conclusion == "failure":
        return FAILED

    if conclusion == "cancelled":
        if jobs is not None and jobs > 0:
            return CANCELLED_BY_HUMAN_OR_PLATFORM
        if jobs == 0:
            if newer_run_exists(run=run, runs=runs):
                return SUPERSEDED_WHILE_QUEUED
            return CANCELLED_BY_HUMAN_OR_PLATFORM
        return NOT_PROVEN

    return NOT_PROVEN


def queue_disposition(run: Mapping[str, Any], terminal_state: str) -> str:
    if terminal_state == QUEUED_LATEST_CANDIDATE:
        return "queued_latest"
    if terminal_state == SUPERSEDED_WHILE_QUEUED:
        return "superseded_before_start"
    if run.get("status") == "in_progress":
        return "started"
    if terminal_state == COMPLETED_EXACT_SUBJECT:
        return "started"
    if terminal_state in {FAILED, CANCELLED_BY_HUMAN_OR_PLATFORM}:
        jobs = job_count(run)
        if jobs is not None and jobs > 0:
            return "started"
        return "not_observed"
    return "not_observed"


def latest_head_relationship(
    *,
    head_sha: str | None,
    latest_head_sha: str,
    terminal_state: str,
) -> str:
    if not isinstance(head_sha, str):
        return "not_observed"
    if head_sha == latest_head_sha:
        return "matches_latest_head"
    if terminal_state == COMPLETED_EXACT_SUBJECT:
        return "exact_subject_not_latest_head"
    if terminal_state == SUPERSEDED_WHILE_QUEUED:
        return "older_head"
    return "older_head"


def displacement_measurement(
    classified_runs: Sequence[Mapping[str, Any]],
    *,
    matrix_legs: int = 2,
) -> dict[str, Any]:
    """Conservative avoided-work estimate for coalesced queued heads."""
    superseded = [
        entry
        for entry in classified_runs
        if entry.get("terminal_state") == SUPERSEDED_WHILE_QUEUED
    ]
    preserved_active = any(
        entry.get("terminal_state") == COMPLETED_EXACT_SUBJECT
        for entry in classified_runs
    )
    return {
        "superseded_while_queued_count": len(superseded),
        "estimated_matrix_legs_avoided": len(superseded) * matrix_legs,
        "active_run_reached_terminal_while_newer_heads_arrived": preserved_active
        and len(superseded) > 0,
        "measurement_basis": (
            "queued supersession counts with explicit job_count==0; "
            "zero-job cancelled runs without a newer same-PR smoke run stay "
            "cancelled_by_human_or_platform; does not replay historical "
            "cancel-in-progress waste or prove manual vs platform cancellation"
        ),
    }


def observe_pull_request_snapshot(
    runs: Sequence[Mapping[str, Any]],
    *,
    pull_request_number: int,
    latest_head_sha: str,
    observed_at: str | None = None,
) -> dict[str, Any]:
    """Build the multi-push observation receipt for one pull request."""
    smoke_runs = [
        run
        for run in runs
        if is_smoke_workflow_run(run) and pull_number(run) == pull_request_number
    ]
    smoke_runs = sorted(smoke_runs, key=lambda item: _run_id(item) or 0)

    classified: list[dict[str, Any]] = []
    for run in smoke_runs:
        terminal_state = classify_terminal_state(
            run, runs=smoke_runs, latest_head_sha=latest_head_sha
        )
        head_sha = run.get("head_sha")
        classified.append(
            {
                "run_id": _run_id(run),
                "head_sha": head_sha,
                "workflow_status": run.get("status"),
                "workflow_conclusion": run.get("conclusion"),
                "job_count": job_count(run),
                "terminal_state": terminal_state,
                "queue_disposition": queue_disposition(run, terminal_state),
                "latest_head_relationship": latest_head_relationship(
                    head_sha=head_sha if isinstance(head_sha, str) else None,
                    latest_head_sha=latest_head_sha,
                    terminal_state=terminal_state,
                ),
            }
        )

    queued_latest = [
        entry["run_id"]
        for entry in classified
        if entry["terminal_state"] == QUEUED_LATEST_CANDIDATE
    ]

    return {
        "schema_version": OBSERVATION_SCHEMA,
        "receipt_kind": OBSERVATION_KIND,
        "issue": "#6887",
        "pull_request": pull_request_number,
        "latest_observed_head_sha": latest_head_sha,
        "concurrency_group": concurrency_group_for_pull(pull_request_number),
        "observed_at": observed_at
        or datetime.datetime.now(datetime.timezone.utc)
        .replace(microsecond=0)
        .isoformat(),
        "runs": classified,
        "measurement": displacement_measurement(classified),
        "merge_verdict_authority": MERGE_VERDICT_AUTHORITY,
        "limitations": [
            "Observation is derived from workflow-run snapshots, not from in-job receipts.",
            "A run that never appears in the snapshot is not proven either way.",
            "Completed exact-subject results do not authorize merge after the head moves.",
        ],
    }


def _validate_classified_run(entry: Mapping[str, Any]) -> None:
    state = entry.get("terminal_state")
    if state not in TERMINAL_STATES:
        raise ValueError(f"invalid terminal_state: {state!r}")
    status = entry.get("workflow_status")
    conclusion = entry.get("workflow_conclusion")
    jobs = entry.get("job_count")
    if not isinstance(status, str):
        raise ValueError("workflow_status must be a string")
    if state == COMPLETED_EXACT_SUBJECT:
        if status != "completed" or conclusion != "success":
            raise ValueError("completed_exact_subject inconsistent with workflow fields")
        if not isinstance(jobs, int) or jobs <= 0:
            raise ValueError("completed_exact_subject requires positive job_count")
    elif state == FAILED:
        if conclusion != "failure":
            raise ValueError("failed inconsistent with workflow_conclusion")
    elif state == SUPERSEDED_WHILE_QUEUED:
        if status != "completed" or conclusion != "cancelled" or jobs != 0:
            raise ValueError("superseded_while_queued inconsistent with workflow fields")
    elif state == CANCELLED_BY_HUMAN_OR_PLATFORM:
        if status != "completed" or conclusion != "cancelled":
            raise ValueError("cancelled_by_human_or_platform inconsistent with workflow fields")
    elif state == QUEUED_LATEST_CANDIDATE:
        if status not in {"queued", "pending", "waiting", "requested"}:
            raise ValueError("queued_latest_candidate inconsistent with workflow_status")


def validate_observation_receipt(payload: Mapping[str, Any]) -> None:
    required_top = (
        "schema_version",
        "receipt_kind",
        "pull_request",
        "latest_observed_head_sha",
        "concurrency_group",
        "runs",
        "measurement",
        "merge_verdict_authority",
    )
    for key in required_top:
        if key not in payload:
            raise ValueError(f"missing {key}")
    if payload["schema_version"] != OBSERVATION_SCHEMA:
        raise ValueError("unexpected schema_version")
    if payload["receipt_kind"] != OBSERVATION_KIND:
        raise ValueError("unexpected receipt_kind")
    if payload["merge_verdict_authority"] != MERGE_VERDICT_AUTHORITY:
        raise ValueError("merge verdict authority drift")
    pull = payload["pull_request"]
    if not isinstance(pull, int) or pull <= 0:
        raise ValueError("pull_request must be a positive integer")
    latest_head = payload["latest_observed_head_sha"]
    if not isinstance(latest_head, str) or len(latest_head) != 40:
        raise ValueError("latest_observed_head_sha must be a 40-char string")
    group = payload["concurrency_group"]
    if not isinstance(group, str) or group != concurrency_group_for_pull(pull):
        raise ValueError("concurrency_group does not match pull_request")
    measurement = payload["measurement"]
    if not isinstance(measurement, dict):
        raise ValueError("measurement must be an object")
    runs = payload["runs"]
    if not isinstance(runs, list):
        raise ValueError("runs must be a list")
    for entry in runs:
        if not isinstance(entry, dict):
            raise ValueError("run entry must be an object")
        _validate_classified_run(entry)


def load_observation_fixture(path: str) -> dict[str, Any]:
    with open(path, encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("fixture must be a JSON object")
    return payload


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--validate-fixture",
        metavar="PATH",
        help="Validate a checked-in observation receipt fixture",
    )
    args = parser.parse_args()
    if args.validate_fixture:
        payload = load_observation_fixture(args.validate_fixture)
        validate_observation_receipt(payload)
        print(f"valid: {args.validate_fixture}")
        return 0
    parser.error("no command selected")
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
