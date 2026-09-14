#!/usr/bin/env python3
"""Fail-closed finalizer for the existing CI Gate dependency aggregate."""

from __future__ import annotations

import json
import os
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
) -> Verdict:
    """Classify the aggregate without inferring success from absent evidence."""
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
        return Verdict("failure", "applicable dependency did not succeed", blockers)
    return Verdict("success", "all applicable dependencies succeeded")


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
        )
        summary = render_summary(raw_needs, verdict)
    except (json.JSONDecodeError, ValueError) as error:
        verdict = Verdict("failure", f"aggregate input was malformed: {error}")
        summary = f"### CI Gate aggregate\n\nVerdict: **failure** — {verdict.reason}.\n"

    if summary_path:
        Path(summary_path).write_text(summary, encoding="utf-8")
    print(summary, end="")
    return 0 if verdict.status in {"success", "scoped_noop"} else 1


if __name__ == "__main__":
    raise SystemExit(main())
