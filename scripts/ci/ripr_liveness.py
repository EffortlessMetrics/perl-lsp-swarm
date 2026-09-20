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


# Events for which `ripr.yml`'s group expression resolves to a pull request
# number rather than to the ref.
PULL_REQUEST_EVENTS = frozenset({"pull_request", "pull_request_target"})


def groups_by_pull_request(run: dict[str, Any]) -> bool:
    """Whether this run's concurrency group is keyed on a pull request number.

    ``ripr.yml`` groups on ``github.event.pull_request.number || github.ref``,
    so the answer follows the event, not the API's ``pull_requests`` array. The
    distinction matters because that array comes back **empty for fork pull
    requests** — the head repository differs from the base — even though the
    run is tied to a real, numbered pull request.
    """
    return run.get("event") in PULL_REQUEST_EVENTS


def base_ref(run: dict[str, Any]) -> str | None:
    """The single base branch this run's pull request targets, if known.

    Filled by the snapshot from a resolution call; absent when the run offered
    a number directly (no resolution needed) or when resolution failed. More
    than one base means the head is shared by several pull requests, which is
    the very ambiguity the caller must not paper over, so that reads as
    unknown rather than as a pick.
    """
    refs = run.get("base_refs")
    if isinstance(refs, list) and len(refs) == 1 and isinstance(refs[0], str) and refs[0]:
        return refs[0]
    return None


def fork_identity(run: dict[str, Any]) -> tuple[str, str, str] | None:
    """The (head repository, head branch, base ref) triple identifying a fork's
    pull request.

    Used only when neither the API nor the resolution call supplied a number.
    Branch alone is not an identity -- two unrelated forks both push
    ``patch-1``. Head repository plus branch is not one either, which is the
    correction #16109 review found: GitHub allows one open pull request per
    head **and base** pair, so a single fork branch can carry two open pull
    requests at once, one onto ``main`` and one onto ``master``. This
    repository's workflows support both, and those two pull requests have
    different numbers and therefore different ``ripr-<pr>`` concurrency
    groups. Matching them would explain one pull request's stall with the
    other's run and suppress a real ``infra-no-proof``.

    All three halves are required. ``None`` when any is missing, which keeps an
    unidentifiable run from matching anything -- the loud failure, an explained
    wait that should have been reported, rather than the silent one.
    """
    repository = run.get("head_repository")
    branch = run.get("head_branch")
    base = base_ref(run)
    if (
        isinstance(repository, str)
        and repository
        and isinstance(branch, str)
        and branch
        and base
    ):
        return (repository, branch, base)
    return None


def resolved_pulls_from_api(returncode: int, stdout: str | None) -> list[dict[str, Any]] | None:
    """Pull requests a ``gh api /commits/<sha>/pulls`` read established.

    ``None`` means the read failed or did not parse, which leaves the run
    without a resolved identity and so matching nothing. Only a list of
    objects is an answer; anything else is unreadable rather than an empty
    result, because reading a garbled body as "no pull requests" would turn a
    resolvable run into an unidentifiable one.
    """
    if returncode != 0 or stdout is None:
        return None
    try:
        parsed = json.loads(stdout)
    except (TypeError, ValueError):
        return None
    if not isinstance(parsed, list):
        return None
    return [entry for entry in parsed if isinstance(entry, dict)]


def same_concurrency_group(left: dict[str, Any], right: dict[str, Any]) -> bool:
    """Whether two runs would contend for the same ``concurrency`` group.

    Grouping follows the event, because ``ripr.yml``'s group expression does:
    ``ripr-<pr>`` for a pull-request run. The number is therefore the identity,
    and everything below is about recovering it when the API withheld it, which
    it does for a fork pull request.

    Two pull-request runs match on a shared number whenever both have one —
    supplied by the API, or filled in by the snapshot's resolution call. Only
    when neither has a number does the ``fork_identity`` triple apply, and it
    requires the base ref precisely because head repository plus branch is not
    an identity: one fork branch can carry two open pull requests at once, one
    onto ``main`` and one onto ``master``, with two different numbers and two
    different concurrency groups.

    A run that offers no identity matches nothing. That is the deliberate
    direction: an unmatched run is reported ``infra-no-proof`` when it was
    merely queued, which is a visible false red on an advisory check, whereas a
    wrong match explains a genuinely dead gate away and nobody ever sees it.
    """
    left_by_pr, right_by_pr = groups_by_pull_request(left), groups_by_pull_request(right)
    if left_by_pr != right_by_pr:
        return False
    if left_by_pr:
        left_pulls, right_pulls = pull_numbers(left), pull_numbers(right)
        # One side knowing its number is enough to decide, and it decides
        # against: a run with a number that the other does not share is a
        # different pull request. The fallback is for when neither side has
        # one, which is the only case its safety argument covers.
        if left_pulls or right_pulls:
            return bool(set(left_pulls) & set(right_pulls))
        left_fork, right_fork = fork_identity(left), fork_identity(right)
        return left_fork is not None and left_fork == right_fork
    branch = left.get("head_branch")
    return bool(branch) and branch == right.get("head_branch")


def predecessor_for(run: dict[str, Any], runs: list[dict[str, Any]]) -> int | None:
    """The newest earlier run holding this run's concurrency group.

    A run claims its concurrency group on admission, not at first job start,
    so ``in_progress`` is too narrow a test: a run whose jobs exist but are
    all waiting on a busy runner pool reads ``queued`` and is holding the
    group regardless. Requiring ``in_progress`` reported the newest head as
    ``infra-no-proof`` during exactly the runner backlog that causes the wait
    (#16109 review). What distinguishes a holder from a sibling that explains
    nothing is whether it has any job at all, which the snapshot already
    reads.

    The candidate must also be older than this run. ``predecessor`` is the
    word used in the posted title and summary -- "queued behind an earlier
    run" -- and without the ordering a genuinely dead run reclassifies to an
    explained wait the moment its *replacement* starts, naming a successor
    that reports on a different head and will never produce proof for this
    one.
    """
    run_id = run.get("id")
    if not isinstance(run_id, int):
        return None
    candidates = [
        candidate_id
        for candidate in runs
        if isinstance(candidate_id := candidate.get("id"), int)
        and candidate_id < run_id
        and candidate.get("status") != "completed"
        and isinstance(candidate.get("job_count"), int)
        and candidate.get("job_count", 0) > 0
        and same_concurrency_group(run, candidate)
    ]
    return max(candidates) if candidates else None


def job_count_from_api(returncode: int, stdout: str | None) -> int | None:
    """The scheduled-job count a ``gh api .total_count`` read established.

    ``None`` means the count could not be read, which the classifier treats as
    scheduled. Only a body that actually parses as a number is a count, so a
    successful call returning nothing — a blank or truncated body — is
    unreadable rather than a confirmed zero. Reading it as zero would turn a
    garbled response into a posted failure on a healthy run.
    """
    if returncode != 0 or stdout is None:
        return None
    try:
        count = int(stdout.strip())
    except (TypeError, ValueError):
        return None
    return count if count >= 0 else None


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
                "event": run.get("event"),
                "head_repository": run.get("head_repository"),
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
