#!/usr/bin/env python3
"""Dispatch one GitHub Actions workflow and prove its exact terminal result.

The release orchestrator uses this helper at workflow boundaries where an
existing publisher is still independently dispatchable. A downstream job may
become reachable only after this helper identifies exactly one new run for the
expected source SHA and observes a successful terminal conclusion.

This is deliberately stricter than `gh workflow run && gh run watch`: the
selected run is bound to repository, workflow, event, source SHA, ref, run ID,
and attempt. Ambiguous or missing runs fail closed.
"""

from __future__ import annotations

import argparse
import dataclasses
import datetime as dt
import json
import os
import subprocess
import sys
import time
from collections.abc import Callable, Iterable, Mapping, Sequence
from typing import Any

TERMINAL_NON_SUCCESS = {
    "action_required",
    "cancelled",
    "failure",
    "neutral",
    "skipped",
    "stale",
    "startup_failure",
    "timed_out",
}


class GateError(RuntimeError):
    """The exact child workflow could not be proven successful."""


@dataclasses.dataclass(frozen=True)
class RunIdentity:
    run_id: int
    run_attempt: int
    workflow_id: int
    event: str
    head_sha: str
    head_branch: str | None
    status: str
    conclusion: str | None
    html_url: str
    created_at: str

    @classmethod
    def from_json(cls, raw: Mapping[str, Any]) -> "RunIdentity":
        try:
            return cls(
                run_id=int(raw["id"]),
                run_attempt=int(raw.get("run_attempt", 1)),
                workflow_id=int(raw["workflow_id"]),
                event=str(raw["event"]),
                head_sha=str(raw["head_sha"]),
                head_branch=(str(raw["head_branch"]) if raw.get("head_branch") else None),
                status=str(raw["status"]),
                conclusion=(str(raw["conclusion"]) if raw.get("conclusion") else None),
                html_url=str(raw["html_url"]),
                created_at=str(raw["created_at"]),
            )
        except (KeyError, TypeError, ValueError) as error:
            raise GateError(f"malformed workflow-run payload: {error}") from error


def _parse_time(value: str) -> dt.datetime:
    try:
        parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as error:
        raise GateError(f"invalid workflow-run created_at value {value!r}") from error
    if parsed.tzinfo is None:
        raise GateError(f"workflow-run created_at lacks timezone: {value!r}")
    return parsed


def select_new_exact_run(
    runs: Iterable[RunIdentity],
    *,
    prior_ids: set[int],
    expected_sha: str,
    dispatch_started: dt.datetime,
) -> RunIdentity | None:
    """Return one exact newly-created run, or fail when selection is ambiguous."""

    eligible = [
        run
        for run in runs
        if run.run_id not in prior_ids
        and run.event == "workflow_dispatch"
        and run.head_sha == expected_sha
        and _parse_time(run.created_at) >= dispatch_started - dt.timedelta(seconds=60)
    ]
    if len(eligible) > 1:
        identities = ", ".join(str(run.run_id) for run in eligible)
        raise GateError(
            "multiple new workflow_dispatch runs match the expected source SHA "
            f"{expected_sha}: {identities}"
        )
    return eligible[0] if eligible else None


def validate_terminal_run(
    run: RunIdentity,
    *,
    expected_sha: str,
    expected_workflow_id: int,
) -> None:
    if run.workflow_id != expected_workflow_id:
        raise GateError(
            f"workflow mismatch: expected {expected_workflow_id}, got {run.workflow_id}"
        )
    if run.event != "workflow_dispatch":
        raise GateError(f"event mismatch: expected workflow_dispatch, got {run.event}")
    if run.head_sha != expected_sha:
        raise GateError(f"source mismatch: expected {expected_sha}, got {run.head_sha}")
    if run.status != "completed":
        raise GateError(f"run {run.run_id} is not terminal: status={run.status}")
    if run.conclusion != "success":
        conclusion = run.conclusion or "missing"
        raise GateError(f"run {run.run_id} did not succeed: conclusion={conclusion}")


def _run_gh(args: Sequence[str], *, expect_json: bool = True) -> Any:
    command = ["gh", *args]
    completed = subprocess.run(
        command,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if completed.returncode != 0:
        stderr = completed.stderr.strip()
        raise GateError(f"gh command failed ({completed.returncode}): {stderr}")
    if not expect_json:
        return None
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise GateError("gh command returned malformed JSON") from error


def _workflow(repo: str, workflow: str) -> Mapping[str, Any]:
    raw = _run_gh(["api", f"repos/{repo}/actions/workflows/{workflow}"])
    if not isinstance(raw, dict):
        raise GateError("workflow lookup did not return an object")
    return raw


def _runs(repo: str, workflow_id: int) -> list[RunIdentity]:
    raw = _run_gh(
        [
            "api",
            "--method",
            "GET",
            f"repos/{repo}/actions/workflows/{workflow_id}/runs",
            "-f",
            "event=workflow_dispatch",
            "-f",
            "per_page=50",
        ]
    )
    if not isinstance(raw, dict) or not isinstance(raw.get("workflow_runs"), list):
        raise GateError("workflow-run listing is malformed")
    return [RunIdentity.from_json(item) for item in raw["workflow_runs"]]


def _run(repo: str, run_id: int) -> RunIdentity:
    raw = _run_gh(["api", f"repos/{repo}/actions/runs/{run_id}"])
    if not isinstance(raw, dict):
        raise GateError("workflow-run lookup did not return an object")
    return RunIdentity.from_json(raw)


def _dispatch(repo: str, workflow_id: int, ref: str, fields: Sequence[str]) -> None:
    args = [
        "api",
        "--method",
        "POST",
        f"repos/{repo}/actions/workflows/{workflow_id}/dispatches",
        "-f",
        f"ref={ref}",
    ]
    for field in fields:
        if "=" not in field or field.startswith("="):
            raise GateError(f"invalid workflow input field {field!r}; expected key=value")
        key, value = field.split("=", 1)
        args.extend(["-f", f"inputs[{key}]={value}"])
    _run_gh(args, expect_json=False)


def _write_output(name: str, value: str) -> None:
    output_path = os.environ.get("GITHUB_OUTPUT")
    if not output_path:
        return
    with open(output_path, "a", encoding="utf-8") as output:
        output.write(f"{name}={value}\n")


def dispatch_and_wait(
    *,
    repo: str,
    workflow: str,
    ref: str,
    expected_sha: str,
    fields: Sequence[str],
    timeout_seconds: int,
    poll_seconds: int,
    clock: Callable[[], float] = time.monotonic,
    sleeper: Callable[[float], None] = time.sleep,
) -> RunIdentity:
    if not expected_sha or len(expected_sha) != 40 or any(
        character not in "0123456789abcdef" for character in expected_sha
    ):
        raise GateError("expected SHA must be a lowercase 40-character hexadecimal commit")
    if timeout_seconds <= 0 or poll_seconds <= 0:
        raise GateError("timeout and poll interval must be positive")

    workflow_raw = _workflow(repo, workflow)
    try:
        workflow_id = int(workflow_raw["id"])
    except (KeyError, TypeError, ValueError) as error:
        raise GateError("workflow lookup lacks a numeric id") from error

    before = _runs(repo, workflow_id)
    prior_ids = {run.run_id for run in before}
    dispatch_started = dt.datetime.now(dt.timezone.utc)
    _dispatch(repo, workflow_id, ref, fields)

    deadline = clock() + timeout_seconds
    selected: RunIdentity | None = None
    while clock() < deadline:
        selected = select_new_exact_run(
            _runs(repo, workflow_id),
            prior_ids=prior_ids,
            expected_sha=expected_sha,
            dispatch_started=dispatch_started,
        )
        if selected is not None:
            break
        sleeper(poll_seconds)
    if selected is None:
        raise GateError(
            f"no exact new run appeared for {workflow} at source {expected_sha}"
        )

    while clock() < deadline:
        current = _run(repo, selected.run_id)
        if current.status == "completed":
            validate_terminal_run(
                current,
                expected_sha=expected_sha,
                expected_workflow_id=workflow_id,
            )
            return current
        sleeper(poll_seconds)

    raise GateError(f"run {selected.run_id} did not reach a terminal state before timeout")


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", required=True)
    parser.add_argument("--workflow", required=True)
    parser.add_argument("--ref", required=True)
    parser.add_argument("--expected-sha", required=True)
    parser.add_argument("--field", action="append", default=[])
    parser.add_argument("--timeout-seconds", type=int, default=14_400)
    parser.add_argument("--poll-seconds", type=int, default=15)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        run = dispatch_and_wait(
            repo=args.repo,
            workflow=args.workflow,
            ref=args.ref,
            expected_sha=args.expected_sha,
            fields=args.field,
            timeout_seconds=args.timeout_seconds,
            poll_seconds=args.poll_seconds,
        )
    except GateError as error:
        print(f"::error::{error}", file=sys.stderr)
        return 1

    _write_output("run_id", str(run.run_id))
    _write_output("run_attempt", str(run.run_attempt))
    _write_output("workflow_id", str(run.workflow_id))
    _write_output("head_sha", run.head_sha)
    _write_output("head_branch", run.head_branch or "")
    _write_output("html_url", run.html_url)
    _write_output("conclusion", run.conclusion or "")
    print(
        f"Exact workflow gate passed: workflow={args.workflow} run={run.run_id} "
        f"attempt={run.run_attempt} sha={run.head_sha}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
