#!/usr/bin/env python3
"""Diagnose required-check registration against an exact subject SHA (#16469).

GitHub branch protection and rulesets require named status contexts. A PR can
look green while remaining unmergeable when:

- a required name was never reported on the classified SHA (#14612), typically
  because a workflow was skipped by path/branch/message filters;
- a required name was reported only on another SHA;
- every required name is a GitHub-accepted conclusion on the classified SHA,
  but GitHub merge evaluation still reports expected checks (#16469).

A job skipped by a conditional reports `skipped` and GitHub treats that as a
successful required-check conclusion. That is not a registration failure.
`product_not_run` records that product work did not run.

This classifier is fail-closed. Missing, partial, or ambiguous evidence is
NOT_PROVEN. COMPLETE_GREEN is a subject-observation verdict, not a protected
merge verdict: GitHub evaluates the test merge commit when that commit has
checks. It never synthesizes merge-ready from absent checks, and it does not
infer a GitHub defect from a blocked merge alone.

Input JSON (stdin or --fixture):

{
  "head_sha": "<40-char lowercase or mixed hex>",
  "required_contexts": [
    {"name": "Perl LSP Rust Small Result", "app_id": 15368}
  ],
  "check_runs": [
    {
      "name": "...",
      "head_sha": "...",
      "status": "completed",
      "conclusion": "success",
      "app": {"id": 15368}
    }
  ],
  "evaluated_sha": "<optional; GitHub-evaluated subject, usually the test merge commit>",
  "merge_evaluation": {
    "mergeable": "MERGEABLE",
    "merge_state_status": "CLEAN",
    "blocked_by_expected_checks": false,
    "blocked_reason": null,
    "merge_commit_sha": "<optional test-merge commit SHA>"
  }
}

GitHub required-check identity is (name, app/integration id). String-only
`required_contexts` and check-runs without a proven `app.id` are NOT_PROVEN,
never a matching success.

`required_contexts` may be omitted when --policy points at
`.ci/policies/required-checks.toml`; required `[[checks]]` names and their
`ruleset_integration_id` values are then the denominator.

Exit 0 only for COMPLETE_GREEN. Every other verdict exits 1.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path
from typing import Any


SUCCESS = {"success", "neutral", "skipped"}
RED = {"failure", "timed_out", "startup_failure", "cancelled", "action_required"}
PRODUCT_SKIPPED = {"skipped"}
EXPECTED_REASON = re.compile(
    r"(required status checks? are expected|checks are expected|expected[ —-]+waiting for status)",
    re.IGNORECASE,
)
SHA_RE = re.compile(r"^[0-9a-fA-F]{40}$")

REMEDY = {
    "COMPLETE_GREEN": (
        "Every required context has a completed GitHub-accepted check-run "
        "(success, skipped, or neutral) from the bound app on the classified "
        "subject SHA, and merge evaluation is not refusing for expected checks. "
        "This is a subject-observation verdict, not a protected merge verdict. "
        "GitHub evaluates the test merge commit when that commit has checks. "
        "A skipped job is accepted registration; product_not_run records that "
        "product work did not run."
    ),
    "REQUIRED_ABSENT": (
        "A required context was never reported on any observed SHA. This is "
        "workflow-level silence (#14612), not a skipped job. A skipped job "
        "emits a check-run and GitHub treats skipped as success. Never treat "
        "absence as success. workflow_dispatch on a PR head does not satisfy "
        "the branch ruleset. Ordinary pull_request/push CI and empty commits "
        "are not authorized while #17900/#17903 G-only qualification is "
        "inactive. Recovery is NOT_PROVEN until a permitted producer "
        "(an admissible event; currently the inactive merge_group/G path) is "
        "live. Check-suite rerequest cannot create a check that was never emitted."
    ),
    "REQUIRED_OTHER_SHA": (
        "A required context is reported, but not on the classified subject SHA. "
        "An older SHA cannot satisfy GitHub's required-check evaluation. "
        "Do not workflow_dispatch the producer against a PR head; that event "
        "does not count for the ruleset. Ordinary PR/push CI and empty commits "
        "are not authorized while #17900/#17903 G-only is inactive."
    ),
    "REQUIRED_RED": (
        "A required context failed on the current head. Repair the failing job; "
        "this is not a registration problem."
    ),
    "REQUIRED_PENDING": (
        "A required context has not reached a terminal result on the current "
        "head. Wait for completion; do not treat pending as green."
    ),
    "EXPECTED_STALE": (
        "Every required context is a GitHub-accepted conclusion (success, "
        "skipped, or neutral) on the classified subject SHA, but GitHub merge "
        "evaluation still reports expected checks (#16469). This is not #14612 "
        "(a required name never reported) and not a skipped job.\n"
        "Do not use a manual dispatch event; GitHub does not count "
        "`workflow_dispatch` toward PR required checks.\n"
        "Do not push an empty commit.\n"
        "Do not start ordinary pull_request/push CI; #17900/#17903 G-only "
        "qualification is inactive.\n"
        "Recovery:\n"
        "1. Confirm this detector reports EXPECTED_STALE, not REQUIRED_ABSENT.\n"
        "2. If an existing check suite was produced by an admissible event "
        "(pull_request, pull_request_target, push), rerequest it: "
        "POST /repos/{owner}/{repo}/check-suites/{id}/rerequest\n"
        "3. If no admissible suite exists, recovery is NOT_PROVEN until the "
        "#17900/#17903 merge_group/G producer is live."
    ),
    "NOT_PROVEN": "Evidence is missing, contradictory, or insufficient.",
}


def required_contexts_from_policy(text: str) -> list[dict[str, Any]]:
    policy = tomllib.loads(text)
    registrations: dict[tuple[str, Any], set[str]] = {}
    for row in policy.get("checks", []):
        if not isinstance(row, dict) or row.get("required") is not True or not row.get("name"):
            continue
        binding = (str(row["name"]), row.get("ruleset_integration_id"))
        events = row.get("events")
        if not isinstance(events, list) or not events:
            raise ValueError("required context event registration is missing")
        for event in events:
            if not isinstance(event, str) or not event:
                raise ValueError("required context event registration is invalid")
            if event in registrations.setdefault(binding, set()):
                raise ValueError(f"required context and event are duplicated: {binding[0]} {event}")
            registrations[binding].add(event)
    rows = [{"name": name, "app_id": app_id} for name, app_id in registrations]
    rows.sort(key=lambda item: str(item["name"]))
    return rows


def required_names_from_policy(text: str) -> tuple[str, ...]:
    return tuple(item["name"] for item in required_contexts_from_policy(text))


def _as_text(value: Any) -> str:
    return value.strip() if isinstance(value, str) else ""


def _positive_id(value: Any) -> int | None:
    if isinstance(value, int) and not isinstance(value, bool) and value > 0:
        return value
    return None


def _run_app_id(run: dict[str, Any]) -> int | None:
    app = run.get("app")
    if not isinstance(app, dict):
        return None
    return _positive_id(app.get("id"))


def _normalize_sha(value: Any) -> str | None:
    text = _as_text(value)
    if not SHA_RE.fullmatch(text):
        return None
    return text.lower()


def _conclusion(run: dict[str, Any]) -> str:
    return _as_text(run.get("conclusion")).lower()


def _status(run: dict[str, Any]) -> str:
    return _as_text(run.get("status")).lower()


def _expected_checks_signal(evaluation: dict[str, Any]) -> bool:
    if evaluation.get("blocked_by_expected_checks") is True:
        return True
    reason = _as_text(evaluation.get("blocked_reason"))
    return bool(reason) and EXPECTED_REASON.search(reason) is not None


def _not_proven(message: str, **extra: Any) -> dict[str, Any]:
    result = {
        "verdict": "NOT_PROVEN",
        "head_status": extra.pop("head_status", "NOT_PROVEN"),
        "required_contexts": extra.pop("required_contexts", []),
        "absent": extra.pop("absent", []),
        "other_sha": extra.pop("other_sha", []),
        "product_not_run": extra.pop("product_not_run", []),
        "red": extra.pop("red", []),
        "pending": extra.pop("pending", []),
        "rows": extra.pop("rows", []),
        "remedy": message,
    }
    result.update(extra)
    return result


def classify(data: Any) -> dict[str, Any]:
    if not isinstance(data, dict):
        return _not_proven("input must be a JSON object")

    head_sha = _normalize_sha(data.get("head_sha"))
    if head_sha is None:
        return _not_proven("head_sha must be a 40-character commit SHA")

    raw_evaluated = data.get("evaluated_sha")
    if raw_evaluated is None or raw_evaluated == "":
        subject_sha = head_sha
    else:
        subject_sha = _normalize_sha(raw_evaluated)
        if subject_sha is None:
            return _not_proven("evaluated_sha must be a 40-character commit SHA")

    required = data.get("required_contexts")
    if not isinstance(required, list) or not required:
        return _not_proven("no required contexts supplied; refuse to invent a denominator")
    bindings: list[tuple[str, int]] = []
    seen: set[tuple[str, int]] = set()
    for item in required:
        if isinstance(item, str):
            return _not_proven(
                "required_contexts entries must be {name, app_id}; "
                "string-only names are not a GitHub required-check identity"
            )
        if not isinstance(item, dict):
            return _not_proven("required_contexts entries must be objects with name and app_id")
        name = _as_text(item.get("name"))
        app_id = _positive_id(item.get("app_id"))
        if not name:
            return _not_proven("required_contexts contains an empty name")
        if app_id is None:
            return _not_proven(
                f"required context {name!r} is missing a positive app_id; "
                "incomplete app evidence is not a matching success"
            )
        key = (name, app_id)
        if key in seen:
            return _not_proven(f"required context is duplicated: {name} app {app_id}")
        seen.add(key)
        bindings.append(key)
    names = [name for name, _app_id in bindings]

    raw_runs = data.get("check_runs")
    if not isinstance(raw_runs, list):
        return _not_proven("check_runs must be an array")

    runs_by_key: dict[tuple[str, int], list[dict[str, Any]]] = {key: [] for key in bindings}
    incomplete_names: set[str] = set()
    bound_names = {name for name, _app_id in bindings}
    for run in raw_runs:
        if not isinstance(run, dict):
            return _not_proven("check_runs entries must be objects")
        name = _as_text(run.get("name"))
        app_id = _run_app_id(run)
        if app_id is None:
            if name in bound_names:
                incomplete_names.add(name)
            continue
        key = (name, app_id)
        if key in runs_by_key:
            runs_by_key[key].append(run)

    absent: list[str] = []
    other_sha: list[str] = []
    product_not_run: list[str] = []
    red: list[str] = []
    pending: list[str] = []
    rows: list[dict[str, Any]] = []

    for name, app_id in bindings:
        observed = runs_by_key[(name, app_id)]
        if not observed:
            if name in incomplete_names:
                return _not_proven(
                    f"required context {name!r} has a same-name run without a proven app.id; "
                    "incomplete app evidence is not a matching success",
                    required_contexts=names,
                )
            absent.append(name)
            rows.append({"name": name, "state": "absent"})
            continue
        on_subject = [
            run for run in observed
            if _normalize_sha(run.get("head_sha")) == subject_sha
        ]
        if not on_subject:
            other_sha.append(name)
            foreign = next(
                (
                    _normalize_sha(run.get("head_sha"))
                    for run in observed
                    if _normalize_sha(run.get("head_sha")) is not None
                ),
                "unknown",
            )
            rows.append({"name": name, "state": "other_sha", "observed_sha": foreign})
            continue

        terminal = [run for run in on_subject if _status(run) == "completed"]
        unfinished = [run for run in on_subject if _status(run) != "completed"]
        if not terminal:
            pending.append(name)
            rows.append({"name": name, "state": "pending"})
            continue

        conclusions = {_conclusion(run) for run in terminal}
        if conclusions & RED:
            red.append(name)
            rows.append({"name": name, "state": "red"})
            continue
        # An unfinished same-subject rerun vetoes an older completed green.
        # check_release_required_ci.py encodes the same rule: latest completed
        # success cannot override a pending or unknown on-subject status.
        if unfinished:
            pending.append(name)
            rows.append({"name": name, "state": "pending"})
            continue
        if conclusions <= SUCCESS:
            if conclusions <= PRODUCT_SKIPPED:
                product_not_run.append(name)
            rows.append({"name": name, "state": "success"})
            continue
        return _not_proven(
            f"required context {name!r} has an unrecognized conclusion {sorted(conclusions)}",
            required_contexts=names,
        )

    def finish(verdict: str, message: str | None = None, **extra: Any) -> dict[str, Any]:
        body = {
            "verdict": verdict,
            "head_status": extra.pop("head_status", "BLOCKED" if verdict != "COMPLETE_GREEN" else "HEAD_GREEN"),
            "required_contexts": names,
            "subject_sha": subject_sha,
            "absent": absent,
            "other_sha": other_sha,
            "product_not_run": extra.pop("product_not_run", product_not_run),
            "red": red,
            "pending": pending,
            "rows": [row for row in rows if row["state"] != "success"],
            "remedy": message or REMEDY[verdict],
        }
        body.update(extra)
        return body

    if absent:
        return finish("REQUIRED_ABSENT", head_status="MISSING")
    if other_sha:
        foreign = next(
            row.get("observed_sha", "unknown") for row in rows if row["state"] == "other_sha"
        )
        return finish(
            "REQUIRED_OTHER_SHA",
            REMEDY["REQUIRED_OTHER_SHA"]
            + f" Classified subject {subject_sha[:12]}; observed {foreign[:12]}.",
            head_status="STALE_SUBJECT",
        )
    if red:
        return finish("REQUIRED_RED", head_status="RED")
    if pending:
        return finish("REQUIRED_PENDING", head_status="PENDING")

    evaluation = data.get("merge_evaluation")
    if evaluation is None:
        return _not_proven(
            "required contexts are success on the exact head, but merge evaluation is missing",
            head_status="HEAD_GREEN",
            required_contexts=names,
            rows=[],
        )
    if not isinstance(evaluation, dict):
        return _not_proven("merge_evaluation must be an object", required_contexts=names)

    mergeable = _as_text(evaluation.get("mergeable")).upper()
    if mergeable == "CONFLICTING":
        return _not_proven(
            "required contexts are success on the exact head, but the merge tree has a conflict",
            head_status="HEAD_GREEN",
            required_contexts=names,
            rows=[],
        )
    if mergeable and mergeable not in {"MERGEABLE", "UNKNOWN"}:
        return _not_proven(
            f"mergeable is {mergeable}; this detector does not classify that state as expected-check registration",
            head_status="HEAD_GREEN",
            required_contexts=names,
            rows=[],
        )

    raw_merge_commit = evaluation.get("merge_commit_sha")
    if raw_merge_commit is not None and raw_merge_commit != "":
        merge_commit = _normalize_sha(raw_merge_commit)
        if merge_commit is None:
            return _not_proven(
                "merge_commit_sha is present but is not a 40-character commit SHA",
                head_status="HEAD_GREEN",
                required_contexts=names,
                rows=[],
            )
        if merge_commit != subject_sha:
            return _not_proven(
                "GitHub evaluates the test merge commit when that commit has checks; "
                f"this observation classified {subject_sha[:12]} while merge_commit_sha "
                f"is {merge_commit[:12]}. Re-run with evaluated_sha set to the merge "
                "commit and check-runs for that SHA. COMPLETE_GREEN is a "
                "subject-observation verdict, not a protected merge verdict.",
                head_status="HEAD_GREEN",
                required_contexts=names,
                rows=[],
            )

    if _expected_checks_signal(evaluation):
        return finish("EXPECTED_STALE", head_status="HEAD_GREEN")

    merge_state = _as_text(evaluation.get("merge_state_status")).upper()
    if mergeable != "MERGEABLE":
        return _not_proven(
            "mergeable is missing or not MERGEABLE; refuse to infer COMPLETE_GREEN",
            head_status="HEAD_GREEN",
            required_contexts=names,
            rows=[],
        )
    if merge_state in {"CLEAN", "BEHIND"}:
        return finish("COMPLETE_GREEN", head_status="HEAD_GREEN")
    if merge_state == "UNSTABLE":
        if evaluation.get("blocked_by_expected_checks") is not False:
            return _not_proven(
                "merge_state_status is UNSTABLE without an expected-checks signal; do not infer a GitHub defect",
                head_status="HEAD_GREEN",
                required_contexts=names,
                rows=[],
            )
        return finish("COMPLETE_GREEN", head_status="HEAD_GREEN")
    if merge_state == "BLOCKED":
        return _not_proven(
            "merge evaluation is blocked without an expected-checks signal; "
            "do not infer a GitHub defect from the merge error alone",
            head_status="HEAD_GREEN",
            required_contexts=names,
            rows=[],
        )
    return _not_proven(
        f"merge_state_status {merge_state or 'missing'} is not classified as expected-check registration",
        head_status="HEAD_GREEN",
        required_contexts=names,
        rows=[],
    )


def _load_required(data: dict[str, Any], policy: Path | None) -> dict[str, Any]:
    if data.get("required_contexts"):
        return data
    if policy is None:
        return data
    data = dict(data)
    data["required_contexts"] = required_contexts_from_policy(policy.read_text(encoding="utf-8"))
    return data


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, help="JSON fixture path; default is stdin")
    parser.add_argument(
        "--policy",
        type=Path,
        help="required-checks.toml used when required_contexts is omitted",
    )
    args = parser.parse_args(argv)
    try:
        raw = args.fixture.read_text(encoding="utf-8") if args.fixture else sys.stdin.read()
        data = json.loads(raw)
        data = _load_required(data, args.policy)
        result = classify(data)
    except (OSError, json.JSONDecodeError, tomllib.TOMLDecodeError, UnicodeError, ValueError) as error:
        result = _not_proven(f"instrument failure: {error}")
    print(json.dumps(result, indent=2, sort_keys=True))
    if result["verdict"] != "COMPLETE_GREEN":
        print(f"{result['verdict']}: {result['remedy']}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
