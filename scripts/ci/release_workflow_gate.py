#!/usr/bin/env python3
"""Retained mechanical child-run leaf; current orchestration does not use it.

Exact selection/terminal identity and a named private observation are necessary
for mechanical success. They never establish release qualification. The CLI has
no artifact identity adapter and fails closed with NOT_PROVEN before dispatch.
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
    repository: str
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
            if any(type(raw.get(key)) is not int or raw[key] <= 0 for key in ("id", "run_attempt", "workflow_id")):
                raise GateError("workflow run numeric identity must be positive exact integers")
            if type(raw.get("repository")) is not dict or not isinstance(raw["repository"].get("full_name"), str):
                raise GateError("workflow run repository identity missing")
            return cls(
                repository=raw["repository"]["full_name"],
                run_id=int(raw["id"]),
                run_attempt=int(raw["run_attempt"]),
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
    expected_repository: str, expected_ref: str, expected_workflow_id: int,
    expected_run_attempt: int, expected_run_id: int | None = None,
) -> RunIdentity | None:
    """Return one exact newly-created run, or fail when selection is ambiguous."""

    eligible = [
        run
        for run in runs
        if run.run_id not in prior_ids
        and run.repository == expected_repository
        and run.head_branch == expected_ref.removeprefix("refs/heads/").removeprefix("refs/tags/")
        and run.workflow_id == expected_workflow_id
        and run.run_attempt == expected_run_attempt
        and (expected_run_id is None or run.run_id == expected_run_id)
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
    expected_repository: str, expected_ref: str, expected_run_id: int, expected_run_attempt: int,
    expected_transaction_id: str, expected_workflow_ref: str, observation_bytes: bytes | None,
) -> None:
    if run.repository != expected_repository:
        raise GateError("repository mismatch")
    if run.head_branch != expected_ref.removeprefix("refs/heads/").removeprefix("refs/tags/"):
        raise GateError("ref mismatch")
    if run.run_id != expected_run_id or run.run_attempt != expected_run_attempt:
        raise GateError("selected run ID/attempt mismatch")
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
    if observation_bytes is None:
        raise GateError("NOT_PROVEN: private observation artifact identity adapter absent")
    import release_publication_admission as admission
    try:
        observation = admission.parse_object(observation_bytes)
    except admission.AdmissionError as error:
        raise GateError("private observation malformed") from error
    if (observation.get("schema_version") != "private_producer_observation.v1"
            or observation.get("phase") != "private_candidate"
            or observation.get("transaction_id") != expected_transaction_id
            or observation.get("source_sha") != expected_sha):
        raise GateError("private observation transaction/source mismatch")
    producer = observation.get("producer")
    if type(producer) is not dict or producer != {"repository": expected_repository,
            "workflow_ref": expected_workflow_ref, "run_id": expected_run_id,
            "run_attempt": expected_run_attempt} or type(producer.get("run_id")) is not int or type(producer.get("run_attempt")) is not int:
        raise GateError("private observation producer identity mismatch")
    if observation.get("qualification") != "not_proven":
        raise GateError("mechanical leaf cannot accept qualification assertions")



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
    expected_run_attempt: int = 1, expected_transaction_id: str = "",
    expected_workflow_ref: str = "", observation_loader: Callable | None = None,
) -> RunIdentity:
    if not expected_sha or len(expected_sha) != 40 or any(
        character not in "0123456789abcdef" for character in expected_sha
    ):
        raise GateError("expected SHA must be a lowercase 40-character hexadecimal commit")
    if timeout_seconds <= 0 or poll_seconds <= 0:
        raise GateError("timeout and poll interval must be positive")

    if observation_loader is None:
        raise GateError("NOT_PROVEN: private observation artifact identity adapter absent")
    if not ref.startswith(("refs/heads/", "refs/tags/")):
        raise GateError("requested ref must name its exact branch/tag namespace")
    if len(expected_transaction_id) != 64 or any(c not in "0123456789abcdef" for c in expected_transaction_id):
        raise GateError("private transaction identity malformed")
    if not expected_workflow_ref.startswith(repo + "/.github/workflows/") or not expected_workflow_ref.endswith("@" + ref):
        raise GateError("expected workflow ref does not bind requested repository/ref")
    if type(expected_run_attempt) is not int or expected_run_attempt <= 0:
        raise GateError("expected run attempt malformed")
    workflow_raw = _workflow(repo, workflow)
    try:
        workflow_id = workflow_raw["id"]
        if type(workflow_id) is not int or workflow_id <= 0:
            raise GateError("workflow ID must be a positive exact integer")
    except (KeyError, TypeError, ValueError) as error:
        raise GateError("workflow lookup lacks a numeric id") from error

    if expected_workflow_ref != repo + "/" + str(workflow_raw.get("path")) + "@" + ref:
        raise GateError("workflow lookup path differs from expected workflow ref")
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
            expected_repository=repo, expected_ref=ref, expected_workflow_id=workflow_id,
            expected_run_attempt=expected_run_attempt,
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
            artifact_name = f"private-producer-{expected_transaction_id}-{expected_run_attempt}"
            artifact = observation_loader(repo, selected.run_id, artifact_name)
            if (type(artifact) is not dict or artifact.get("repository") != repo
                    or artifact.get("run_id") != selected.run_id or artifact.get("name") != artifact_name
                    or type(artifact.get("bytes")) is not bytes):
                raise GateError("NOT_PROVEN: exact private observation artifact absent")
            validate_terminal_run(
                current,
                expected_sha=expected_sha,
                expected_workflow_id=workflow_id, expected_repository=repo, expected_ref=ref,
                expected_run_id=selected.run_id, expected_run_attempt=expected_run_attempt,
                expected_transaction_id=expected_transaction_id, expected_workflow_ref=expected_workflow_ref,
                observation_bytes=artifact["bytes"],
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
        f"Mechanical workflow gate passed; release qualification NOT_PROVEN: workflow={args.workflow} run={run.run_id} "
        f"attempt={run.run_attempt} sha={run.head_sha}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
