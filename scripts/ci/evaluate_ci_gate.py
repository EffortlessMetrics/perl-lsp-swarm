#!/usr/bin/env python3
"""Fail-closed finalizer for the existing CI Gate dependency aggregate."""

from __future__ import annotations

import json
import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping

EXPECTED_DEPENDENCIES = (
    "draft-pr-check",
    "preflight-latest-check",
    "conflict-markers",
    "check-all-targets",
    "ux-tests",
    "merge-gate-shards",
)

SCOPED_NOOP_ALLOWED_SKIPS = frozenset(EXPECTED_DEPENDENCIES) - {
    "draft-pr-check",
}

# The exit contract, named so it can be asserted against rather than
# restated. An allowlist, not a denylist: a status added later cannot reach
# exit 0 merely by existing. `main` is the only consumer.
GREEN_STATUSES = frozenset({"success", "scoped_noop"})


@dataclass(frozen=True)
class Verdict:
    status: str
    reason: str
    blockers: tuple[str, ...] = ()


def _dependency(needs: Mapping[str, Any], name: str) -> Mapping[str, Any] | None:
    value = needs.get(name)
    return value if isinstance(value, Mapping) else None


def _result(needs: Mapping[str, Any], name: str) -> str:
    value = _dependency(needs, name)
    if value is None:
        return "missing"
    result = value.get("result")
    return result if isinstance(result, str) and result else "missing"


def _output(needs: Mapping[str, Any], name: str, output: str) -> str:
    value = _dependency(needs, name)
    outputs = value.get("outputs") if value is not None else None
    if not isinstance(outputs, Mapping):
        return "missing"
    result = outputs.get(output)
    return result if isinstance(result, str) and result else "missing"


def _observed_dependencies(needs: Mapping[str, Any]) -> tuple[str, ...]:
    return tuple(sorted(set(EXPECTED_DEPENDENCIES) | set(needs)))


def _scoped_noop_blockers(needs: Mapping[str, Any]) -> tuple[str, ...]:
    """Reject unexpected outcomes even when a route is intentionally skipped."""
    return tuple(
        f"{name}={result}"
        for name in _observed_dependencies(needs)
        for result in (_result(needs, name),)
        if result != "success"
        and not (
            result == "skipped" and name in SCOPED_NOOP_ALLOWED_SKIPS
        )
    )


def evaluate(
    needs: Mapping[str, Any],
    *,
    event_name: str = "pull_request",
    pull_request_draft: str = "false",
    run_head: str = "",
    latest_head: str = "",
    replacement_run: str = "",
) -> Verdict:
    """Classify the aggregate without inferring success from absent evidence.

    `ci.yml` sets `cancel-in-progress` on pull-request synchronize, so a second
    push cancels the run in flight, its lanes with it, and this job still
    starts under `if: always()` — which is how a routine push reddened the
    aggregate with "applicable dependency did not succeed" (#16087).

    **The fix is the sentence, not the colour.** That is the decision #16087
    called the real one and answered: "exiting 1 keeps a superseded head
    visibly unproven, which is honest... my recommendation is exit 1 with the
    distinguishing message, because 'no proof for this SHA' genuinely is not
    a pass." A cancelled lane produced no evidence, and no evidence is not
    evidence of correctness. So every path below that is not a real success
    exits 1, and what changes is which sentence the reader is handed.

    Three of them, partitioned as the issue asks:

    - a lane that reached a verdict and lost -> `failure`, wording unchanged;
    - every blocking lane cancelled, with a newer run identified ->
      `superseded`, naming the head that replaced this one and the run that
      is authoritative for it;
    - every blocking lane cancelled with no newer run found -> `no_verdict`,
      saying only that, because that is all the inputs support.

    `_all_cancelled(blockers)` is what keeps the first separate: a genuine
    failure alongside a cancellation is still a failure, whatever the heads
    say. A failed lane is information about the code, and code survives a
    head move in a way a cancellation does not.

    `cancelled()` appears nowhere, for two reasons, the second measured
    rather than reasoned. It reports *that* a run was cancelled and not
    *why*, so a maintainer cancelling by hand is indistinguishable from
    concurrency — which is what #5460 refused a pass verdict over. And in
    this job it is simply false: run 35507473500 was concurrency-cancelled
    with every dependency `cancelled`, and the `if: cancelled()` steps in this
    very job were skipped. The run was cancelled; the job was not.

    `ripr.yml` is unaffected. It sets `cancel-in-progress: false`, so a
    cancelled ripr lane never means supersession and its
    `cancelled-no-verdict` block stays correct unchanged.
    """
    draft_result = _result(needs, "draft-pr-check")
    if draft_result != "success":
        return Verdict(
            "failure",
            "draft guard did not complete successfully",
            (f"draft-pr-check={draft_result}",),
        )

    run_ci = _output(needs, "draft-pr-check", "run_ci")
    if run_ci == "false":
        if event_name == "pull_request" and pull_request_draft == "true":
            blockers = _scoped_noop_blockers(needs)
            if blockers:
                return Verdict(
                    "failure",
                    "draft scoped-noop had an unexpected dependency outcome",
                    blockers,
                )
            return Verdict("scoped_noop", "draft pull request")
        return Verdict(
            "failure",
            "non-draft route was not selected",
            ("draft-pr-check.run_ci=false",),
        )
    if run_ci != "true":
        return Verdict(
            "failure",
            "draft guard did not select a route",
            (f"draft-pr-check.run_ci={run_ci}",),
        )

    preflight_result = _result(needs, "preflight-latest-check")
    if preflight_result != "success":
        return Verdict(
            "failure",
            "preflight did not complete successfully",
            (f"preflight-latest-check={preflight_result}",),
        )

    is_latest = _output(needs, "preflight-latest-check", "is_latest")
    if is_latest == "false":
        if event_name == "push":
            blockers = _scoped_noop_blockers(needs)
            if blockers:
                return Verdict(
                    "failure",
                    "superseded scoped-noop had an unexpected dependency outcome",
                    blockers,
                )
            return Verdict("scoped_noop", "superseded push")
        return Verdict(
            "failure",
            "non-push route reported a superseded subject",
            ("preflight-latest-check.is_latest=false",),
        )
    if is_latest != "true":
        return Verdict(
            "failure",
            "preflight did not establish candidate freshness",
            (f"preflight-latest-check.is_latest={is_latest}",),
        )

    observed = _observed_dependencies(needs)
    blockers = tuple(
        f"{name}={_result(needs, name)}"
        for name in observed
        if _result(needs, name) != "success"
    )
    if blockers:
        # #16087's partition: a lane that reached a verdict and lost is a
        # failure; a lane cancelled before it could reach one produced no
        # verdict. Both are red. Only the sentence differs, and the sentence
        # is the entire complaint — "applicable dependency did not succeed" is
        # the same words for a broken test and for a routine second push.
        if _all_cancelled(blockers):
            if _superseded(run_head, latest_head, replacement_run):
                # "is this workflow's run for that head" and not "is
                # authoritative for it". Review objected to the stronger word
                # twice, and on the second reading it is right: the lookup
                # establishes that the run exists, belongs to this pull
                # request, and came after this one. It establishes nothing
                # about that run's outcome — it may itself be cancelled. The
                # sentence is this change's entire deliverable, so it says
                # what was read and stops.
                return Verdict(
                    "superseded",
                    f"no proof for {run_head[:8]}: every blocking lane was "
                    f"cancelled. The pull request has moved to "
                    f"{latest_head[:8]}, and run {replacement_run} is this "
                    f"workflow's run for that head. This is NOT a test "
                    f"failure",
                    blockers,
                )
            return Verdict(
                "no_verdict",
                "no proof for this SHA: every blocking lane was cancelled "
                "before reaching a verdict, and no newer run was identified. "
                "This is NOT a test failure",
                blockers,
            )
        return Verdict("failure", "applicable dependency did not succeed", blockers)
    return Verdict("success", "all applicable dependencies succeeded")


def _all_cancelled(blockers: tuple[str, ...]) -> bool:
    """Whether every blocker is a cancelled lane rather than a real outcome.

    Necessary but not sufficient for `superseded`: one genuine failure
    alongside a cancellation is still a failure, however far the head has
    moved. The head comparison supplies the sufficient half.
    """
    return all(blocker.rsplit("=", 1)[-1] == "cancelled" for blocker in blockers)


_OBJECT_NAME = re.compile(r"[0-9a-f]{40}")
_RUN_ID = re.compile(r"[0-9]+")


def _superseded(run_head: str, latest_head: str, replacement_run: str) -> bool:
    """Whether a newer run has demonstrably replaced the one this tested.

    Three facts, and the third is the one that took two attempts to get
    right. Both heads must be well-formed object names, they must differ,
    **and** the replacement run must be identified.

    Differing heads alone were the earlier design, and the review that
    rejected it was correct: a moved head establishes that the candidate
    changed, not that anything exists to prove the new one. #16087's accepted
    design asks for a demonstrable newer run, so the workflow step looks one
    up by the live head, within this same workflow, and binds its id here.
    Without that id there is no replacement to point at, and "nothing proved
    this candidate, but something else will" is not a claim the inputs
    support.

    Everything unresolved reads as "not superseded" — no token, an API error,
    an event with no pull request, a head that moved before any run started.

    Nothing here turns the gate green any more: both branches exit 1, and
    this only chooses which sentence the reader gets. That is deliberate, and
    it is why the checks below stay strict anyway. A wrong answer here now
    costs a misleading message rather than an unearned pass, and a misleading
    message is the entire defect #16087 is about. Naming a run that is not
    proving this candidate would reproduce that defect with more confidence
    than the generic wording it replaced.

    So a truncated or garbled value reads as "unknown", never as "different,
    therefore newer", and the caller falls back to saying only what it can
    prove: every blocking lane was cancelled, and no newer run was found.
    """
    return (
        _OBJECT_NAME.fullmatch(run_head) is not None
        and _OBJECT_NAME.fullmatch(latest_head) is not None
        and run_head != latest_head
        and _RUN_ID.fullmatch(replacement_run) is not None
    )


def render_summary(needs: Mapping[str, Any], verdict: Verdict) -> str:
    lines = ["### CI Gate aggregate", "", "| Dependency | Result |", "| --- | --- |"]
    for name in sorted(set(EXPECTED_DEPENDENCIES) | set(needs)):
        lines.append(f"| {name} | {_result(needs, name)} |")
    lines.extend(["", f"Verdict: **{verdict.status}** — {verdict.reason}."])
    if verdict.blockers:
        lines.append(f"Blocking evidence: {', '.join(verdict.blockers)}")
    return "\n".join(lines) + "\n"


# GitHub's workflow-command escaping. Only these three characters are
# special in a command's *message*: a literal `%` would be read as the start
# of an escape, and a newline would end the command — which is also how a
# message could smuggle a second command onto the next line.
_COMMAND_ESCAPES = (("%", "%25"), ("\r", "%0D"), ("\n", "%0A"))


def annotation(verdict: Verdict) -> str:
    """The verdict as a workflow annotation, for readers outside the web UI.

    The rendered summary goes to `$GITHUB_STEP_SUMMARY`, which the REST API
    does not serve, and this job's check run carries no `output` body —
    measured on check runs 106217897926 and 106217227911, both empty. So a
    reader on the API (the checks list, tooling, an agent triaging a red)
    sees `Process completed with exit code 1` and nothing else, and a
    superseded head is indistinguishable from a broken test. Annotations are
    the one failure surface REST does serve, which is the same repair #15492
    made for `PR Smoke` (#16198, `74976a651`).

    The text is the verdict already computed, not a second classification.
    """
    # The same two sentences `render_summary` builds, including its trailing
    # period: a reason does not carry one, and without it the blockers clause
    # runs straight into the last word.
    text = f"{verdict.status}: {verdict.reason}."
    if verdict.blockers:
        text = f"{text} Blocking evidence: {', '.join(verdict.blockers)}"
    for raw, escaped in _COMMAND_ESCAPES:
        text = text.replace(raw, escaped)
    return f"::error title=CI Gate aggregate::{text}"


def main() -> int:
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    try:
        raw_needs = json.loads(os.environ.get("NEEDS_JSON", ""))
        if not isinstance(raw_needs, dict):
            raise ValueError("NEEDS_JSON must contain an object")
        verdict = evaluate(
            raw_needs,
            event_name=os.environ.get("EVENT_NAME", ""),
            pull_request_draft=os.environ.get("PULL_REQUEST_DRAFT", ""),
            # Normalised here, shape-checked in `_superseded`. The workflow
            # step exports neither unless it is already 40 hex characters.
            run_head=os.environ.get("RUN_HEAD_SHA", "").strip(),
            latest_head=os.environ.get("LATEST_HEAD_SHA", "").strip(),
            replacement_run=os.environ.get("REPLACEMENT_RUN_ID", "").strip(),
        )
        summary = render_summary(raw_needs, verdict)
    except (json.JSONDecodeError, ValueError) as error:
        verdict = Verdict("failure", f"aggregate input was malformed: {error}")
        summary = f"### CI Gate aggregate\n\nVerdict: **failure** — {verdict.reason}.\n"

    if summary_path:
        Path(summary_path).write_text(summary, encoding="utf-8")
    print(summary, end="")

    # One binding for both the colour and the annotation, so they cannot
    # disagree. An annotation is emitted only on a red verdict: GitHub
    # renders `::error::` as a failure annotation whatever the exit code, so
    # an unconditional one would hang an error off a green check.
    green = verdict.status in GREEN_STATUSES
    if not green:
        print(annotation(verdict))
    # Only a route that was never meant to run is green besides success.
    #
    # `superseded` and `no_verdict` are red, deliberately, and that is the
    # decision #16087 said was the real one: "my recommendation is exit 1
    # with the distinguishing message, because 'no proof for this SHA'
    # genuinely is not a pass." A cancelled lane is absent proof, and absent
    # proof is not evidence of correctness.
    #
    # The cost of exiting 0 instead is not symmetric with the cost of exiting
    # 1. A wrongly red check costs an hour of a pull request looking broken.
    # A wrongly green one is recorded against a SHA and outlives the reason
    # it was granted — a forgiven head that a force-push later restores
    # carries a success nothing ever earned, and no later failure supersedes
    # it. Every defect found on this change was a risk of granting a pass;
    # none of them exists when there is no pass to grant.
    return 0 if green else 1


if __name__ == "__main__":
    raise SystemExit(main())
