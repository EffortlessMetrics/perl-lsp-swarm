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

A fork pull request run held for maintainer approval reads ``waiting`` with no
scheduled jobs. That is a human-actionable gate, not a scheduling fault
(#16151), and reporting it as ``infra-no-proof`` attributes a maintainer
decision to the scheduler and trains the reader that the signal is unreliable.
It is reported separately as ``awaiting_approval`` so the remedy is named
correctly: the absence of proof clears the moment a maintainer approves the
workflow.

Only a run with nothing scheduled, past the floor, and no predecessor to
explain it is ``infra-no-proof`` — the class ``ripr.yml`` already applies to a
lane killed by the runner. Nothing is producing proof and nothing is going to.

``classify_snapshot`` is a pure function of a recorded snapshot, so a decision
is reproducible from its inputs rather than from whatever the API happened to
return while a cron job ran.

What it says when nothing is wrong
----------------------------------

A reporter that only ever reports absence leaves the reader with no way to tell
a working run from a dead one, because the run object is no help: GitHub
advances ``updated_at`` only on run- and job-level transitions, not while a step
executes, and a measured healthy run sat ~40 minutes inside a single step. So
this also reports a run that *is* working, naming the step currently running.

``alive`` is a separate channel from ``findings`` and is structurally incapable
of being a fault. The only ``failure`` this reporter can write is
``infra-no-proof``, and no path reaches it from a run that has jobs.

Withdrawing what it said before
-------------------------------

A check run is GitHub's current state, and this one used to only ever add to
it. A stall that cleared left its red on the head indefinitely, which is
indistinguishable at a glance from a live stall -- the learned ignorance this
reporter exists to prevent.

The memory for that is the API's own record: the ``external_id`` is
``ripr-liveness:<run id>:<classification>``, and the check runs this reporter
wrote are already on the head. Reading them back is what lets a later fire
update the same check run rather than contradict it. Deliberately not a
persisted state file: that would be a second thing to go stale, be evicted, or
be written by a run that then failed before posting. A retraction exists only
for a run this snapshot actually observed -- absence of evidence is not evidence
of recovery.
"""

from __future__ import annotations

import argparse
import datetime
import json
import sys
from pathlib import Path
from typing import Any

# Statuses in which GitHub has accepted a run but scheduled nothing for it.
# `waiting` is the fork-PR approval hold: same observable shape (zero jobs,
# no scheduled work) as a scheduling stall, but a different cause, so it is in
# the set for the snapshot to read job counts on it but gets its own
# classification downstream (#16151).
UNSTARTED_STATUSES = frozenset({"queued", "pending", "waiting", "requested"})

DEFAULT_FLOOR_MINUTES = 10

SCHEDULED = "scheduled"
WITHIN_FLOOR = "within_floor"
SERIALISED = "serialised_behind_predecessor"
AWAITING_APPROVAL = "awaiting_approval"
INFRA_NO_PROOF = "infra-no-proof"
ALIVE = "alive"

REPORTABLE = frozenset({SERIALISED, AWAITING_APPROVAL, INFRA_NO_PROOF, ALIVE})

# Never "success": this reports on the absence of proof and must not be able to
# signal that proof exists. `awaiting_approval` is neutral because the wait is
# a maintainer decision, not a fault the scheduler can recover (#16151).
#
# `alive` is neutral for the same reason from the other direction: it reports
# that a run *has* scheduled work and names the step currently running, which is
# a progress observation and not a verdict about the candidate. It is additive
# and structurally cannot red anything -- the only `failure` in this table is
# `infra-no-proof`, and no path reaches it from a run that has jobs.
CONCLUSIONS = {
    SERIALISED: "neutral",
    AWAITING_APPROVAL: "neutral",
    INFRA_NO_PROOF: "failure",
    ALIVE: "neutral",
}

# The only conclusion a retraction may carry. A resolution exists to withdraw a
# failure the reporter itself wrote, and a `success` would be the exact thing
# this reporter must never say: it knows nothing about the candidate, and a
# withdrawn stall means the scheduler recovered, not that the run proved
# anything.
RESOLUTION_CONCLUSION = "neutral"

# Posting under the required context's own name could change whether a pull
# request is mergeable, and a check run left behind here could outlive the real
# one. The advisory name is deliberately distinct.
REQUIRED_CONTEXT_NAME = "ripr+ New Gap Gate"
ADVISORY_CHECK_NAME = "ripr+ liveness"

# A run is only `alive` while it is actually running. `queued` with jobs is a
# different state -- waiting on a runner, which is what `serialised` exists to
# describe -- and `completed` has nothing left to report.
IN_FLIGHT_STATUSES = frozenset({"in_progress"})

# How long after a run finishes the reporter still considers withdrawing a red
# it wrote for it. This is a valve, not a budget: it drops the older portion of
# the runs page once throughput rises, and it is *not* what bounds the read
# today. Measured on the last 100 `ripr` runs, the page spans 264 minutes and
# carries 91 distinct heads, so a 6-hour bound drops nothing and every head is
# still read. That number is the honest cost of the capability and is reported
# in the step summary rather than assumed away -- the read is what lets a
# retraction happen at all, and shrinking it would silently stop red from ever
# clearing, which is the defect this exists to fix.
#
# It clears the slowest observed run twice over (118 min) so a long run's head
# is never dropped while the run is still going.
POSTED_MEMORY_HOURS = 6


def posted_memory_heads(runs: list[dict[str, Any]], as_of: datetime.datetime | None) -> set[str]:
    """The heads still worth reading this reporter's own history for.

    Two populations qualify, and the second is the one that is easy to forget:
    runs that are still going, and runs that have only just finished. A stall
    is posted against a run that is waiting, but a run that stalls and then
    completes never becomes non-completed again, so a population of "runs that
    have not finished" would never withdraw the red for the commonest recovery
    of all.

    A run that finished longer ago than `POSTED_MEMORY_HOURS` is dropped. This
    is a throughput valve rather than a budget -- see the constant's comment
    for what it does and does not bound today.

    An unreadable clock yields the conservative set -- every head with one --
    because over-reading costs API calls and under-reading silently skips a
    retraction, and the failure modes are not equally bad. A naive datetime is
    treated the same way rather than raising out of the snapshot step: the
    subtraction below is only defined for an aware value, and this is a public
    seam whose annotation does not say so.
    """
    heads = {
        run["head_sha"]
        for run in runs
        if isinstance(run, dict)
        and isinstance(run.get("head_sha"), str)
        and run["head_sha"]
    }
    if as_of is None or as_of.tzinfo is None:
        return heads
    age_limit = POSTED_MEMORY_HOURS * 60
    keep: set[str] = set()
    for run in runs:
        if not isinstance(run, dict):
            continue
        head = run.get("head_sha")
        if not isinstance(head, str) or not head:
            continue
        if run.get("status") != "completed":
            keep.add(head)
            continue
        created_at = parse_time(run.get("created_at"))
        if created_at is None or (as_of - created_at).total_seconds() // 60 <= age_limit:
            keep.add(head)
    return keep

# The one identity a posted check run can be addressed by, across cron fires.
# The run id is the anchor and the classification is the claim, so a recovery
# is a change of conclusion for an id this reporter already wrote rather than a
# new id: `ripr-liveness:35487554523:infra-no-proof` stays the same check run
# and its conclusion moves.
EXTERNAL_ID_PREFIX = "ripr-liveness"


def external_id_for(run_id: Any, classification: str) -> str | None:
    """The ``external_id`` a finding for this run and class is posted under.

    ``None`` when the run id is not an integer, because an id built from an
    unreadable run cannot be retracted later: there would be no stable address
    to withdraw through. Callers treat ``None`` as "not postable" rather than
    coercing the value into a string.

    This lives here rather than in the workflow because the whole transition
    story keys on it. The post loop needs it to decide POST-versus-PATCH, and
    the classification needs it to recognise which of its own earlier writes are
    now stale, so an unposted, unread, or reordered copy of this format would
    silently disable both (#16567).
    """
    if not isinstance(run_id, int) or isinstance(run_id, bool):
        return None
    return f"{EXTERNAL_ID_PREFIX}:{run_id}:{classification}"


def parse_external_id(value: Any) -> tuple[int, str] | None:
    """The ``(run_id, classification)`` an ``external_id`` addresses, if it does.

    Only this reporter's own ids are recognised. A check run on the head written
    by any other job carries someone else's namespace, and reading a run id out
    of it would let this reporter decide the fate of a check run it does not
    own. An unrecognised id is ``None``, which the caller treats as "not mine,
    leave it alone".
    """
    if not isinstance(value, str):
        return None
    parts = value.split(":")
    if len(parts) != 3 or parts[0] != EXTERNAL_ID_PREFIX:
        return None
    run_id_text, classification = parts[1], parts[2]
    if not run_id_text.isdigit():
        return None
    return int(run_id_text), classification


def posted_checks_from_api(returncode: int, stdout: str | None) -> dict[str, int] | None:
    """The ``external_id -> check run id`` map a check-runs read established.

    Maps to the numeric id, not to a set of names, because a resolution has to
    be *addressed*: withdrawing a stall means updating the check run that
    already carries the red, and the check-runs API addresses a write by that
    run's own id. A set would be enough to suppress a duplicate and useless for
    a retraction, which is the same reason the format lives in this module.

    ``None`` means the read failed or nothing in the body was an addressable
    row, which leaves the caller to choose between posting anyway and staying
    silent. The workflow refuses to stay silent, and that refusal stays
    deliberate policy in the workflow text rather than becoming a silent
    default here.

    Each line is one JSON object, which is what ``--jq '.check_runs[] |
    @json'`` emits under ``--paginate``. A row is taken only when it carries a
    usable ``external_id`` **and** a positive integer ``id``: a partial row
    would produce an update aimed at check run 0, and the API's response to
    that is a 404 that looks identical to a row that was never ours.
    """
    if returncode != 0 or stdout is None:
        return None
    posted: dict[str, int] = {}
    for line in stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except (TypeError, ValueError):
            continue
        if not isinstance(row, dict):
            continue
        external_id = row.get("external_id")
        check_id = row.get("id")
        if not isinstance(external_id, str) or not external_id:
            continue
        if not isinstance(check_id, int) or isinstance(check_id, bool) or check_id <= 0:
            continue
        posted[external_id] = check_id
    return posted if posted else None


def posted_id_for(
    posted: dict[str, dict[str, int]],
    head_sha: Any,
    run_id: Any,
    classification: str,
) -> int | None:
    """The check run id already carrying this reporter's claim on this head.

    ``None`` means "not posted yet", which is the caller's cue to POST; an
    integer means "this is the run to update". The distinction is made once,
    here, rather than in the transport, because getting it backwards is exactly
    the defect class this reporter has already shipped once: a dedup read that
    answered a different question than the one it was written for, and looked
    correct while doing it.

    An unreadable head map is treated as not-posted. That direction re-posts a
    check run that may already exist, which is visible and correctable; the
    opposite would silently stop updating a live signal.
    """
    external_id = external_id_for(run_id, classification)
    if external_id is None or not isinstance(head_sha, str):
        return None
    on_head = posted.get(head_sha)
    if not isinstance(on_head, dict):
        return None
    check_id = on_head.get(external_id)
    if isinstance(check_id, int) and not isinstance(check_id, bool) and check_id > 0:
        return check_id
    return None


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


def apply_resolved_identity(run: dict[str, Any], resolved: list[dict[str, Any]] | None) -> None:
    """Record on ``run`` what a ``/commits/<sha>/pulls`` read established.

    This lives here rather than in the workflow because it decides a posted
    check run, and because the defect it exists to prevent is only visible
    across the seam: the endpoint answers "which pull requests contain this
    commit", and the snapshot needs "which pull request triggered this run".
    Those differ, and writing every returned number onto the run silently
    converts the first into the second (#16109 review).

    A head reached by **several** pull requests establishes no identity at
    all. It is not a number to match on and not a base to fall back to: the
    head is genuinely shared, and which of those pull requests this run
    belongs to is exactly what the read failed to settle. Recording the whole
    set instead made two runs triggered by *different* pull requests match on
    a non-empty intersection, and the intersection was tested before the base
    refs were ever consulted, so the base could not rescue it.

    Marked ambiguous rather than left bare, because bare means "no number
    known" and would fall through to the ``fork_identity`` triple — which
    would then match these two runs on head repository and branch, the very
    collision this is closing.
    """
    if resolved is None:
        return
    numbers = sorted({
        entry["number"] for entry in resolved if isinstance(entry.get("number"), int)
    })
    if len(numbers) > 1:
        run["ambiguous_identity"] = True
        return
    if not numbers:
        return
    run["pull_requests"] = numbers
    bases = sorted({
        entry["base"] for entry in resolved
        if isinstance(entry.get("base"), str) and entry.get("base")
    })
    if bases:
        run["base_refs"] = bases


def has_ambiguous_identity(run: dict[str, Any]) -> bool:
    """Whether this run's pull request was never narrowed to one.

    Either the resolution call found the head on several pull requests, or the
    API's own array carried more than one. Both mean the triggering number is
    unknown, and an unknown number must match nothing.
    """
    return run.get("ambiguous_identity") is True or len(pull_numbers(run)) > 1


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

    An **ambiguous** identity is not a weak identity, it is none: a head on
    two pull requests yields two numbers, and a set intersection would call
    two runs from two different pull requests a match. That check is refused
    before either the number or the fork triple is consulted, because both
    would accept it.
    """
    left_by_pr, right_by_pr = groups_by_pull_request(left), groups_by_pull_request(right)
    if left_by_pr != right_by_pr:
        return False
    if left_by_pr:
        if has_ambiguous_identity(left) or has_ambiguous_identity(right):
            return False
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


def progress_from_api(returncode: int, stdout: str | None) -> dict[str, Any] | None:
    """The step-level detail a ``gh api .../jobs`` read established for a run.

    ``None`` means unreadable, which the caller reports as an ``alive`` line
    with no step named rather than as nothing at all. Dropping the run instead
    would make a transient read failure look exactly like a run that is not
    making progress, which is the confusion this reporter exists to remove.

    ``in_progress_steps`` are the names of steps currently running, in the order
    the API returned them. Only that and the completed/total counts are read:
    every number here is something GitHub asserts about the run, so a reader can
    check it. Nothing here infers a percentage of the work, because a run's
    step list is not a plan and "3/4 steps" would be a guess dressed as a fact.
    """
    if returncode != 0 or stdout is None:
        return None
    try:
        parsed = json.loads(stdout)
    except (TypeError, ValueError):
        return None
    if not isinstance(parsed, dict):
        return None
    total = parsed.get("total_count")
    if not isinstance(total, int) or isinstance(total, bool) or total < 0:
        return None
    completed = parsed.get("completed_count", 0)
    if not isinstance(completed, int) or isinstance(completed, bool) or completed < 0:
        completed = 0
    steps = parsed.get("in_progress_steps")
    names = [name for name in steps if isinstance(name, str) and name] if isinstance(steps, list) else []
    return {
        "total_count": total,
        "completed_count": min(completed, total),
        "in_progress_steps": names,
    }


def alive_title(run: dict[str, Any], elapsed_minutes: int, progress: dict[str, Any] | None) -> str:
    """One line naming what an in-flight run is doing right now.

    The run object is motionless for most of a healthy run's life -- GitHub
    advances ``updated_at`` only on run- and job-level transitions, not while a
    step executes, and a measured healthy run sat ~40 minutes inside one step.
    So the run object carries no evidence either way, and the step list is the
    only thing that does. That is the whole reason this line exists.
    """
    run_id = run.get("id")
    if not progress:
        return f"ripr run {run_id} alive: {elapsed_minutes} min elapsed, no step detail"
    steps = progress.get("in_progress_steps") or []
    completed = progress.get("completed_count", 0)
    total = progress.get("total_count", 0)
    jobs = f"{completed}/{total} jobs done"
    if steps:
        running = " + ".join(f'"{name}"' for name in steps)
        return f"ripr run {run_id} alive: {jobs}, running {running}, {elapsed_minutes} min"
    return f"ripr run {run_id} alive: {jobs}, no step running, {elapsed_minutes} min"


def alive_summary(run: dict[str, Any], elapsed_minutes: int, progress: dict[str, Any] | None) -> str:
    """The body behind the alive title.

    States the observation and what it does not mean. The last paragraph is the
    load-bearing one: a reader who finds a green-ish `neutral` on this check
    must not be able to mistake it for proof, because the reporter has no
    opinion about the candidate and never has.
    """
    run_id = run.get("id")
    head = run.get("head_sha", "unknown")
    lines = [
        f"Run `{run_id}` on `{head}` has been running {elapsed_minutes} minutes "
        "and has scheduled work.",
        "",
    ]
    if progress:
        steps = progress.get("in_progress_steps") or []
        lines.append(
            f"{progress.get('completed_count', 0)} of "
            f"{progress.get('total_count', 0)} scheduled jobs are complete."
        )
        if steps:
            lines.append("")
            lines.append("Currently running: " + ", ".join(f"`{name}`" for name in steps) + ".")
    else:
        lines.append(
            "The step-level read did not return, so what the run is doing inside "
            "a step is not shown here. The run is still in progress; this is a "
            "gap in the detail, not evidence of a stall."
        )
    lines += [
        "",
        "This is a progress observation, not a verdict. It is not the required "
        "context, it evaluates no candidate, and it must not be read as the run "
        "having produced proof.",
    ]
    return "\n".join(lines) + "\n"


def classify_run(
    run: dict[str, Any],
    runs: list[dict[str, Any]],
    waited_minutes: int,
    floor_minutes: int,
) -> tuple[str, int | None]:
    job_count = run.get("job_count")
    # A run with jobs is the case this reporter historically said nothing about,
    # which is the half of the gap that costs an auditor: silence for a working
    # run and a permanent red for a recovered one are the same defect, and only
    # the second is a fault.
    #
    # `alive` says the run is in progress, which `status` alone establishes.
    # The step detail is a refinement, not a condition: a run whose jobs read
    # failed is still alive, and the caller renders it as an alive line naming
    # no step rather than dropping it. Dropping it would make a transient read
    # failure indistinguishable from a run making no progress, which is the
    # exact confusion this reporter exists to remove.
    if run.get("status") in IN_FLIGHT_STATUSES:
        # Past the floor, like every other class here. A run two minutes old
        # needs no heartbeat, and the floor is the rule this reporter already
        # uses to decide what is worth saying: measured over the last 100 ripr
        # runs, 18 were in flight at any moment, and a line on each of them
        # every 15 minutes would be ~1,700 advisory writes a day saying a run
        # had just started. The reader who needs this is the one asking whether
        # a long run is dead, and the floor is exactly that threshold.
        if waited_minutes < floor_minutes:
            return SCHEDULED, None
        return ALIVE, None
    # An unreadable job count is not zero: treat it as scheduled rather than
    # report a run whose emptiness was never established.
    if job_count is None or not isinstance(job_count, int) or job_count > 0:
        return SCHEDULED, None
    if run.get("status") not in UNSTARTED_STATUSES:
        return SCHEDULED, None
    # A run whose status is `waiting` is held for fork-PR maintainer approval
    # rather than queued for execution (#16151). The approval hold is a
    # human-actionable gate, not an infrastructure fault: name it so the
    # remedy is obvious, and skip predecessor lookup because no run can be
    # holding a slot for an unapproved fork PR.
    if run.get("status") == "waiting":
        if waited_minutes < floor_minutes:
            return WITHIN_FLOOR, None
        return AWAITING_APPROVAL, None
    if waited_minutes < floor_minutes:
        return WITHIN_FLOOR, None
    predecessor = predecessor_for(run, runs)
    if predecessor is not None:
        return SERIALISED, predecessor
    return INFRA_NO_PROOF, None


def check_title(classification: str, waited_minutes: int) -> str:
    if classification == SERIALISED:
        return f"ripr queued behind an earlier run for {waited_minutes} min"
    if classification == AWAITING_APPROVAL:
        return f"ripr awaiting fork-PR approval for {waited_minutes} min"
    return f"ripr has scheduled no jobs for {waited_minutes} min"


# Why a stall this reporter previously reported is no longer one, keyed by what
# was positively observed to have changed. A retraction names only a reason it
# can point at; the fallback branch is deliberately absent, because an unproven
# reason is a sentence about a live CI run that nothing backs up.
RESOLVED = "it has scheduled jobs now"
RESOLVED_RUNNING = "it is running"
RESOLVED_QUEUED = "it is queued behind an earlier run, which is a designed wait"
RESOLVED_APPROVAL = "it is held for fork-PR maintainer approval"
RESOLVED_COMPLETED = "the run has completed"

RESOLUTION_BODIES = {
    RESOLVED: "Jobs are scheduled on it, so the absence of proof that was "
    "reported has ended.",
    RESOLVED_RUNNING: "The run is in progress, so it is no longer waiting with "
    "nothing scheduled.",
    RESOLVED_QUEUED: "An earlier ripr run on this pull request is now holding "
    "the concurrency group, which is the designed `cancel-in-progress: false` "
    "wait rather than a scheduling fault.",
    RESOLVED_APPROVAL: "The run is held for fork-PR maintainer approval, which "
    "is a human gate rather than an infrastructure fault.",
    RESOLVED_COMPLETED: "The run has finished, so the wait this reported is over.",
}


def recovery_reason(
    run: dict[str, Any],
    classification: str,
) -> str | None:
    """Why a reported stall no longer holds -- or ``None`` when it may still hold.

    This is the difference between *recovered* and *not known to be recovered*,
    and the whole retraction path turns on it.

    ``classify_run`` maps an **unreadable** job count to ``SCHEDULED``, which
    is right for reporting -- an unreadable count is not a confirmed zero -- and
    exactly wrong for retraction. Treating "I could not read this" as "the
    condition cleared" withdraws the red from a run that is still genuinely
    dead, and the next fire re-reds it: the head flaps neutral/failure every 15
    minutes while the gate is down. That is a worse failure than the one this
    reporter exists to prevent, because it looks like the fix working.

    So a retraction needs positive evidence, in one of exactly four shapes: a
    count that reads above zero, a run that is in progress, a run that finished,
    or a classification that names a different cause. Everything else -- an
    unreadable count, a run back inside the floor, a run still empty -- is
    silence.
    """
    if classification == ALIVE:
        return RESOLVED_RUNNING
    if classification == SERIALISED:
        return RESOLVED_QUEUED
    if classification == AWAITING_APPROVAL:
        return RESOLVED_APPROVAL
    if run.get("status") == "completed":
        return RESOLVED_COMPLETED
    job_count = run.get("job_count")
    if isinstance(job_count, int) and not isinstance(job_count, bool) and job_count > 0:
        return RESOLVED
    return None


def resolution_title(run: dict[str, Any], reason: str) -> str:
    """One line withdrawing a stall this reporter previously reported."""
    return f"ripr run {run.get('id')} no longer stalled: {reason}"


def resolution_summary(run: dict[str, Any], reason: str) -> str:
    """The body behind a retraction, saying plainly what changed and why."""
    run_id = run.get("id")
    head = run.get("head_sha", "unknown")
    lines = [
        f"The `infra-no-proof` failure this reporter posted for run `{run_id}` on "
        f"`{head}` no longer holds. What it reads as now: {reason}.",
        "",
        RESOLUTION_BODIES[reason],
        "",
        "This withdraws a claim this reporter made about scheduling. It is not a "
        "statement that the run produced proof: no conclusion here evaluates the "
        "candidate, and the required context is unaffected.",
    ]
    return "\n".join(lines) + "\n"


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
    elif classification == AWAITING_APPROVAL:
        lines += [
            "This run is held because it was triggered from a fork pull "
            "request and is awaiting maintainer approval on the Actions tab. "
            "No job will schedule until a maintainer approves the workflow "
            "for this pull request; the wait is by design and the remedy is a "
            "single human click, not a scheduler intervention.",
            "",
            "Reporting this as `infra-no-proof` would attribute a maintainer "
            "decision to the scheduler and degrade the signal this reporter "
            "exists to provide, so it is named `awaiting_approval` instead.",
        ]
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


def classify_snapshot(
    snapshot: dict[str, Any],
    posted_checks: dict[str, dict[str, int]] | None = None,
) -> dict[str, Any]:
    """Classify one snapshot against what this reporter has already posted.

    ``posted_checks`` maps a head SHA to the ``external_id -> check run id``
    pairs found on it, or is ``None``/absent when the caller could not read
    them. It is the reporter's memory across cron fires, and it comes from the
    API rather than from a file: the check runs this reporter wrote *are* the
    record, and a persisted state file would be a second thing to go stale, get
    evicted, or be written by a run that then failed before posting.

    With no memory the report is exactly what it always was, minus the two
    transitions. A missing read therefore costs a retraction and a heartbeat
    update, never a wrong conclusion, which is the right direction for the one
    call whose failure mode would be a false statement about a run.
    """
    as_of = parse_time(snapshot.get("as_of"))
    floor = snapshot.get("floor_minutes")
    floor_minutes = floor if isinstance(floor, int) and floor > 0 else DEFAULT_FLOOR_MINUTES
    runs = snapshot.get("runs")
    runs = runs if isinstance(runs, list) else []
    posted = posted_checks if isinstance(posted_checks, dict) else {}
    heads_read = sum(1 for value in posted.values() if isinstance(value, dict))

    findings: list[dict[str, Any]] = []
    alive: list[dict[str, Any]] = []
    resolutions: list[dict[str, Any]] = []
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

        # A stall this reporter already reported for this run, on this head, is
        # withdrawn the moment the run positively stops reading as one. The
        # external id is the same one the failure was posted under, so this is
        # an update to that check run and not a second, competing statement
        # about it.
        run_id = run.get("id")
        head_sha = run.get("head_sha")
        stall_id = external_id_for(run_id, INFRA_NO_PROOF)
        posted_check_id = posted_id_for(posted, head_sha, run_id, INFRA_NO_PROOF)
        reason = recovery_reason(run, classification)
        if posted_check_id is not None and reason is not None:
            resolutions.append(
                {
                    "run_id": run_id,
                    "head_sha": head_sha,
                    "check_name": ADVISORY_CHECK_NAME,
                    "check_run_id": posted_check_id,
                    "external_id": stall_id,
                    "conclusion": RESOLUTION_CONCLUSION,
                    "now_classification": classification,
                    "reason": reason,
                    "check_title": resolution_title(run, reason),
                    "check_summary": resolution_summary(run, reason),
                }
            )

        if classification == ALIVE:
            # Reported through its own channel. A run that is working is not a
            # finding about absent proof, and folding it into `findings` would
            # make every existing reader of that list -- the post loop, the
            # markdown table, the "no run is waiting" early exit -- start
            # handling a fact they were never written for.
            progress = run.get("progress")
            alive.append(
                {
                    "run_id": run_id,
                    "head_sha": head_sha,
                    "head_branch": run.get("head_branch"),
                    "pull_requests": pull_numbers(run),
                    "event": run.get("event"),
                    "status": run.get("status"),
                    "elapsed_minutes": waited_minutes,
                    "classification": ALIVE,
                    "check_name": ADVISORY_CHECK_NAME,
                    "conclusion": CONCLUSIONS[ALIVE],
                    "external_id": external_id_for(run_id, ALIVE),
                    "check_run_id": posted_id_for(posted, head_sha, run_id, ALIVE),
                    "progress": progress if isinstance(progress, dict) else None,
                    "check_title": alive_title(run, waited_minutes, progress),
                    "check_summary": alive_summary(run, waited_minutes, progress),
                }
            )
            continue

        if classification not in REPORTABLE:
            continue
        findings.append(
            {
                "run_id": run_id,
                "head_sha": head_sha,
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
                "external_id": external_id_for(run_id, classification),
                "check_run_id": posted_id_for(posted, head_sha, run_id, classification),
                "check_title": check_title(classification, waited_minutes),
                "check_summary": check_summary(
                    classification, run, waited_minutes, predecessor
                ),
            }
        )

    return {
        "schema_version": 2,
        "kind": "ripr_liveness_report",
        "as_of": snapshot.get("as_of"),
        "floor_minutes": floor_minutes,
        "runs_examined": len(runs),
        "heads_read": heads_read,
        "findings": findings,
        "alive": alive,
        "resolutions": resolutions,
        "unclassifiable": unclassifiable,
        "claim_boundary": [
            "Reports only that a run has scheduled no jobs; it evaluates no candidate.",
            "Never emits a success conclusion, and never posts under the required context's name.",
            "A run queued behind an earlier run on the same pull request is an explained wait, not a fault.",
            "A run held for fork-PR maintainer approval is a human-actionable wait, not a scheduling fault (#16151).",
            "A run whose created_at, the snapshot's as_of, or whose job count is unreadable is reported for neither.",
            "An `alive` line is a progress observation, not a verdict, and is the only class reachable from a run that has jobs.",
            "A resolution withdraws a stall this reporter itself posted, and only for a run this snapshot actually observed.",
            "A run absent from the snapshot is never retracted: no observation means no claim, not a clear one.",
        ],
    }


def render(report: dict[str, Any]) -> str:
    findings = report.get("findings", [])
    alive = report.get("alive", [])
    resolutions = report.get("resolutions", [])
    lines = [
        "### ripr liveness",
        "",
        f"{report.get('runs_examined', 0)} run(s) examined, floor "
        f"{report.get('floor_minutes', DEFAULT_FLOOR_MINUTES)} min, "
        f"{report.get('heads_read', 0)} head(s) read for prior state.",
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
    if alive:
        lines += ["", "**Running now**", ""]
        for entry in alive:
            lines.append(f"- {entry.get('check_title')}")
    if resolutions:
        lines += ["", "**Stalls withdrawn since the last report**", ""]
        for entry in resolutions:
            lines.append(
                f"- run `{entry.get('run_id')}`: was reported "
                f"`{INFRA_NO_PROOF}`, now `{entry.get('now_classification')}`"
            )
    unclassifiable = report.get("unclassifiable", [])
    if unclassifiable:
        lines += [
            "",
            f"{len(unclassifiable)} run(s) could not be classified for want of a "
            "readable timestamp; see the JSON report.",
        ]
    return "\n".join(lines) + "\n"


def read_posted(path: Path) -> dict[str, dict[str, int]] | None:
    """The head-keyed check-run map the snapshot step read, or ``None``.

    A missing or unreadable file is ``None`` rather than ``{}`` for the same
    reason a failed read is: the classifier treats "no memory" as "cannot
    retract, cannot tell an update from a first post", and both of those fail
    toward re-posting rather than toward silence.
    """
    try:
        parsed = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, TypeError, ValueError):
        return None
    return parsed if isinstance(parsed, dict) else None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", required=True, type=Path)
    parser.add_argument("--out", type=Path, default=Path("target/ripr/liveness/report.md"))
    parser.add_argument("--json", type=Path, default=Path("target/ripr/liveness/report.json"))
    parser.add_argument(
        "--posted",
        type=Path,
        default=Path("target/ripr/liveness/posted.json"),
        help="head_sha -> {external_id: check run id} read from the check-runs API",
    )
    parser.add_argument("--print", action="store_true", dest="print_report")
    args = parser.parse_args(argv)

    snapshot = json.loads(args.snapshot.read_text(encoding="utf-8"))
    posted = read_posted(args.posted) if args.posted.exists() else None
    report = classify_snapshot(snapshot, posted)
    markdown = render(report)

    for path, body in ((args.json, json.dumps(report, indent=2) + "\n"), (args.out, markdown)):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body, encoding="utf-8")
    if args.print_report:
        sys.stdout.write(markdown)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
