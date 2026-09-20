#!/usr/bin/env python3
"""Report a ripr run that is sitting with no jobs scheduled.

The gap
-------

``ripr.yml`` serialises per pull request: ``concurrency: ripr-<pr>`` with
``cancel-in-progress: false``, deliberately, so a newer head queues behind an
active analysis instead of turning an infrastructure cancellation into a false
red required check.

The cost is that a queued run schedules **no jobs at all**, and GitHub posts a
check run only for a scheduled job. The required context ``ripr+ New Gap
Gate`` is therefore not red and not pending on that head: it is absent. Branch
protection renders that the same way it renders a check still running, so a run
that will never produce proof and one three minutes from finishing look
identical to a reader.

Measured 2026-09-20: run 35487554523 (PR #16083, head ``2698f30``) sat
``pending`` with ``jobs.total_count == 0`` for 47 minutes while 74 other check
runs reported on that head. ``ripr+ New Gap Gate`` was not among them.

What this reports, and what it refuses to
-----------------------------------------

A run queued behind an earlier run **on the same pull request** is the designed
behaviour, not a fault. Reporting it as a failure would red a pull request for
waiting its turn, which is the defect class this exists to reduce. It is
reported as an explained wait, naming the predecessor, so the silence is broken
without inventing a fault.

Only a run with nothing scheduled, past the floor, and no predecessor to
explain it is ``infra-no-proof`` — the class ``ripr.yml`` already applies to a
lane killed by the runner. Nothing is producing proof and nothing is going to.

``classify_snapshot`` is a pure function of a recorded snapshot, so a decision
is reproducible from its inputs rather than from whatever the API happened to
return while a cron job ran.
"""

from __future__ import annotations

import argparse
import datetime
import json
import sys
from pathlib import Path
from typing import Any

# Statuses in which GitHub has accepted a run but scheduled nothing for it.
UNSTARTED_STATUSES = frozenset({"queued", "pending", "waiting", "requested"})

DEFAULT_FLOOR_MINUTES = 10

SCHEDULED = "scheduled"
WITHIN_FLOOR = "within_floor"
SERIALISED = "serialised_behind_predecessor"
INFRA_NO_PROOF = "infra-no-proof"

REPORTABLE = frozenset({SERIALISED, INFRA_NO_PROOF})

# Never "success": this reports on the absence of proof and must not be able to
# signal that proof exists.
CONCLUSIONS = {
    SERIALISED: "neutral",
    INFRA_NO_PROOF: "failure",
}

# Posting under the required context's own name could change whether a pull
# request is mergeable, and a check run left behind here could outlive the real
# one. The advisory name is deliberately distinct.
REQUIRED_CONTEXT_NAME = "ripr+ New Gap Gate"
ADVISORY_CHECK_NAME = "ripr+ liveness"


def parse_time(value: Any) -> datetime.datetime | None:
    if not isinstance(value, str):
        return None
    try:
        parsed = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return None
    return parsed.astimezone(datetime.timezone.utc)


def pull_numbers(run: dict[str, Any]) -> list[int]:
    pulls = run.get("pull_requests")
    if not isinstance(pulls, list):
        return []
    return [pull for pull in pulls if isinstance(pull, int)]


def same_concurrency_group(left: dict[str, Any], right: dict[str, Any]) -> bool:
    """Whether two runs would contend for the same ``concurrency`` group.

    ``ripr.yml`` groups by pull request number, falling back to the ref, so two
    runs share a group when they share a pull request or, for a push, a branch.
    """
    left_pulls, right_pulls = pull_numbers(left), pull_numbers(right)
    if left_pulls and right_pulls:
        return bool(set(left_pulls) & set(right_pulls))
    if not left_pulls and not right_pulls:
        branch = left.get("head_branch")
        return bool(branch) and branch == right.get("head_branch")
    return False


def predecessor_for(run: dict[str, Any], runs: list[dict[str, Any]]) -> int | None:
    """The newest started run holding this run's concurrency group.

    Only a run that actually started can be occupying the group; a sibling that
    is itself queued explains nothing.
    """
    run_id = run.get("id")
    candidates = [
        candidate.get("id")
        for candidate in runs
        if candidate.get("id") != run_id
        and candidate.get("status") == "in_progress"
        and same_concurrency_group(run, candidate)
        and isinstance(candidate.get("id"), int)
    ]
    return max(candidates) if candidates else None


def classify_run(
    run: dict[str, Any],
    runs: list[dict[str, Any]],
    waited_minutes: int,
    floor_minutes: int,
) -> tuple[str, int | None]:
    job_count = run.get("job_count")
    # An unreadable job count is not zero: treat it as scheduled rather than
    # report a run whose emptiness was never established.
    if job_count is None or not isinstance(job_count, int) or job_count > 0:
        return SCHEDULED, None
    if run.get("status") not in UNSTARTED_STATUSES:
        return SCHEDULED, None
    if waited_minutes < floor_minutes:
        return WITHIN_FLOOR, None
    predecessor = predecessor_for(run, runs)
    if predecessor is not None:
        return SERIALISED, predecessor
    return INFRA_NO_PROOF, None


def check_title(classification: str, waited_minutes: int) -> str:
    if classification == SERIALISED:
        return f"ripr queued behind an earlier run for {waited_minutes} min"
    return f"ripr has scheduled no jobs for {waited_minutes} min"


def check_summary(
    classification: str,
    run: dict[str, Any],
    waited_minutes: int,
    predecessor: int | None,
) -> str:
    run_id = run.get("id")
    head = run.get("head_sha", "unknown")
    lines = [
        f"Run `{run_id}` on `{head}` has been waiting {waited_minutes} minutes "
        "with no jobs scheduled.",
        "",
        "A run with no scheduled job posts no check run, so "
        f"`{REQUIRED_CONTEXT_NAME}` is **absent** from this head rather than "
        "red or pending. Branch protection reports that the same way it "
        "reports a check that is still running.",
        "",
    ]
    if classification == SERIALISED:
        lines.append(
            "This is the designed behaviour, not a fault. `ripr.yml` sets "
            "`cancel-in-progress: false` so a newer head queues behind an "
            f"active analysis instead of cancelling it, and run `{predecessor}` "
            "on this pull request is still producing evidence. Nothing to do; "
            "the gate will report once that run finishes."
        )
    else:
        lines += [
            "No earlier ripr run on this pull request is in progress, so "
            "nothing explains the wait and nothing is producing proof for this "
            f"head: `{INFRA_NO_PROOF}`, the class `ripr.yml` already applies to "
            "a lane killed by the runner.",
            "",
            f"Recovery: `gh run rerun {run_id}`, or push to re-trigger.",
        ]
    lines += [
        "",
        "_This check reports only that no job was scheduled. It evaluates no "
        "candidate, never reports success, and is not the required context._",
    ]
    return "\n".join(lines) + "\n"


def classify_snapshot(snapshot: dict[str, Any]) -> dict[str, Any]:
    as_of = parse_time(snapshot.get("as_of"))
    floor = snapshot.get("floor_minutes")
    floor_minutes = floor if isinstance(floor, int) and floor > 0 else DEFAULT_FLOOR_MINUTES
    runs = snapshot.get("runs")
    runs = runs if isinstance(runs, list) else []

    findings: list[dict[str, Any]] = []
    unclassifiable: list[dict[str, Any]] = []

    for run in runs:
        if not isinstance(run, dict):
            continue
        if as_of is None:
            # Without a readable clock reading there is no elapsed time, and a
            # guessed one would decide a real check run.
            unclassifiable.append(
                {"run_id": run.get("id"), "reason": "snapshot carries no readable as_of"}
            )
            continue
        created_at = parse_time(run.get("created_at"))
        if created_at is None:
            unclassifiable.append(
                {"run_id": run.get("id"), "reason": "run carries no readable created_at"}
            )
            continue

        waited_minutes = int((as_of - created_at).total_seconds() // 60)
        classification, predecessor = classify_run(run, runs, waited_minutes, floor_minutes)
        if classification not in REPORTABLE:
            continue
        findings.append(
            {
                "run_id": run.get("id"),
                "head_sha": run.get("head_sha"),
                "head_branch": run.get("head_branch"),
                "pull_requests": pull_numbers(run),
                "status": run.get("status"),
                "job_count": run.get("job_count"),
                "waited_minutes": waited_minutes,
                "classification": classification,
                "check_name": ADVISORY_CHECK_NAME,
                "conclusion": CONCLUSIONS[classification],
                "predecessor_run_id": predecessor,
                "check_title": check_title(classification, waited_minutes),
                "check_summary": check_summary(
                    classification, run, waited_minutes, predecessor
                ),
            }
        )

    return {
        "schema_version": 1,
        "kind": "ripr_liveness_report",
        "as_of": snapshot.get("as_of"),
        "floor_minutes": floor_minutes,
        "runs_examined": len(runs),
        "findings": findings,
        "unclassifiable": unclassifiable,
        "claim_boundary": [
            "Reports only that a run has scheduled no jobs; it evaluates no candidate.",
            "Never emits a success conclusion, and never posts under the required context's name.",
            "A run queued behind an earlier run on the same pull request is an explained wait, not a fault.",
            "A run whose created_at, the snapshot's as_of, or whose job count is unreadable is reported for neither.",
        ],
    }


def render(report: dict[str, Any]) -> str:
    findings = report.get("findings", [])
    lines = [
        "### ripr liveness",
        "",
        f"{report.get('runs_examined', 0)} run(s) examined, floor "
        f"{report.get('floor_minutes', DEFAULT_FLOOR_MINUTES)} min.",
        "",
    ]
    if not findings:
        lines.append("Every run has scheduled jobs or is still inside the floor.")
    else:
        lines += [
            "| run | head | waited | classification | predecessor |",
            "|---|---|---:|---|---|",
        ]
        for finding in findings:
            head = (finding.get("head_sha") or "unknown")[:7]
            predecessor = finding.get("predecessor_run_id")
            lines.append(
                f"| `{finding.get('run_id')}` | `{head}` | "
                f"{finding.get('waited_minutes')} min | "
                f"{finding.get('classification')} | "
                f"{'`%s`' % predecessor if predecessor else 'none'} |"
            )
    unclassifiable = report.get("unclassifiable", [])
    if unclassifiable:
        lines += [
            "",
            f"{len(unclassifiable)} run(s) could not be classified for want of a "
            "readable timestamp; see the JSON report.",
        ]
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", required=True, type=Path)
    parser.add_argument("--out", type=Path, default=Path("target/ripr/liveness/report.md"))
    parser.add_argument("--json", type=Path, default=Path("target/ripr/liveness/report.json"))
    parser.add_argument("--print", action="store_true", dest="print_report")
    args = parser.parse_args(argv)

    snapshot = json.loads(args.snapshot.read_text(encoding="utf-8"))
    report = classify_snapshot(snapshot)
    markdown = render(report)

    for path, body in ((args.json, json.dumps(report, indent=2) + "\n"), (args.out, markdown)):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body, encoding="utf-8")
    if args.print_report:
        sys.stdout.write(markdown)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
