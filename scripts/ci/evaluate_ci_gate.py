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

    Forgiving that needs **positive evidence that a newer candidate exists**,
    which is `latest_head != run_head`: the pull request's live head is no
    longer the head this run tested, so a replacement run is already proving
    the thing this one stopped proving. Nothing weaker will do, and in
    particular `cancelled()` will not.

    It will not for two reasons, and the second was measured rather than
    reasoned. It reports *that* a run was cancelled and not *why*, so a
    maintainer cancelling by hand is indistinguishable from concurrency —
    which is what #5460 refused a pass verdict over, and it was right. And in
    this job it is simply false: run 35507473500 was concurrency-cancelled
    with every dependency `cancelled`, and the `if: cancelled()` steps in this
    very job were skipped. The run was cancelled; the job was not. So no
    cancellation fact reaches this function at all, and none is needed.

    What remains necessary is `_all_cancelled(blockers)`: a genuine failure
    alongside a cancellation is still a failure, whatever the heads say. The
    head comparison supplies the rest.

    `ripr.yml` is unaffected either way. It sets `cancel-in-progress: false`,
    so a cancelled ripr lane never means supersession and its
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
        if _all_cancelled(blockers):
            if _superseded(run_head, latest_head, replacement_run):
                return Verdict(
                    "superseded",
                    f"cancelled run for {run_head[:8]} superseded by "
                    f"{latest_head[:8]}, proved by run {replacement_run}",
                    blockers,
                )
            # Cancelled lanes with no newer head. #16107 named this state and
            # kept it red — NOT_PROVEN for a SHA nothing proved — and that
            # rule is carried here unchanged. It gets no status of its own:
            # the needs map cannot tell a cancelled run from a cancelled job.
            # Run 35507473500 was concurrency-cancelled and still reported
            # three green lanes, so "every lane cancelled" is not the shape of
            # a cancelled run, and no other cancellation evidence reaches this
            # function. A verdict naming a cause it cannot establish is the
            # error #16186's first attempt made; `failure` is what is provable
            # and it blocks identically.
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

    Everything unresolved reads as "not superseded" and the gate stays red —
    no token, an API error, an event with no pull request, a head that moved
    before any run started. That is the direction to fail in: a missed
    supersession costs one avoidable red, while a wrongly claimed one reports
    green over a candidate nothing proved.

    The workflow step already refuses to export a malformed value. These
    checks repeat it because this is the one path that can turn the gate
    green, and a shell condition in a YAML file is a thin single layer to
    rest that on. A truncated or garbled value must read as "unknown", never
    as "different, therefore newer".
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
    # `superseded` exits 0 because a cancelled run proved nothing about the
    # candidate and a newer run is already proving it. `scoped_noop` is the
    # same argument for a route that was never meant to run. Every other
    # status is red, `failure` over cancelled lanes included: that is the
    # superseded shape with the replacement missing, which is absent proof
    # and not a pass. This is an allowlist rather than a denylist, so a
    # status added later cannot reach exit 0 merely by existing.
    return 0 if verdict.status in {"success", "scoped_noop", "superseded"} else 1


if __name__ == "__main__":
    raise SystemExit(main())
