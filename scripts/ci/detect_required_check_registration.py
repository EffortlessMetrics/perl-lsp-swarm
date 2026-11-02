#!/usr/bin/env python3
"""Diagnose required-check registration against an exact PR head (#16469).

GitHub branch protection and rulesets require named status contexts. A PR can
look green while remaining unmergeable when:

- a required name was never reported on the current head (#14612);
- a required name was reported only on another SHA;
- a required job ran as skipped, which does not satisfy a required context;
- every required name is success on the exact head, but GitHub merge
  evaluation still reports expected checks (#16469).

This classifier is fail-closed. Missing, partial, or ambiguous evidence is
NOT_PROVEN. It never synthesizes merge-ready from absent checks, and it does
not infer a GitHub defect from a blocked merge alone.

Input JSON (stdin or --fixture):

{
  "head_sha": "<40-char lowercase or mixed hex>",
  "required_contexts": ["Perl LSP Rust Small Result", "..."],
  "check_runs": [
    {"name": "...", "head_sha": "...", "status": "completed", "conclusion": "success"}
  ],
  "merge_evaluation": {
    "mergeable": "MERGEABLE",
    "merge_state_status": "CLEAN",
    "blocked_by_expected_checks": false,
    "blocked_reason": null
  }
}

`required_contexts` may be omitted when --policy points at
`.ci/policies/required-checks.toml`; the required `[[checks]]` names are then
the denominator.

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


SUCCESS = {"success"}
RED = {"failure", "timed_out", "startup_failure", "cancelled", "action_required"}
SKIPPED = {"skipped", "neutral"}
PENDING_STATUS = {"queued", "in_progress", "waiting", "requested", "pending"}
EXPECTED_REASON = re.compile(
    r"(required status checks? are expected|checks are expected|expected[ —-]+waiting for status)",
    re.IGNORECASE,
)
SHA_RE = re.compile(r"^[0-9a-fA-F]{40}$")

REMEDY = {
    "COMPLETE_GREEN": (
        "Every required context has a completed success check-run on the exact "
        "head, and merge evaluation is not refusing for expected checks."
    ),
    "REQUIRED_ABSENT": (
        "A required context was never reported on any observed SHA. This is the "
        "#14612 shape (workflow did not produce a check-run), not #16469. "
        "Never treat absence as success. Dispatch the producing workflow against "
        "the current head (`workflow_dispatch` with that exact SHA) and wait for "
        "a check-run whose name matches the required context."
    ),
    "REQUIRED_OTHER_SHA": (
        "A required context is reported, but not on the current head SHA. An "
        "older SHA cannot satisfy GitHub's required-check evaluation. Re-run or "
        "dispatch the producing workflow against the current head."
    ),
    "REQUIRED_SKIPPED": (
        "A required context reported skipped/neutral on the current head. GitHub "
        "does not treat skipped required checks as success, so the merge box "
        "stays on Expected. Convert the emitting job to always-or-scoped-noop: "
        "keep the job reachable and no-op inside the job when the work is out of "
        "scope. Do not change the live ruleset from this detector."
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
        "Every required context is completed success on the exact current head, "
        "but GitHub merge evaluation still reports expected checks (#16469). "
        "This is not #14612 (a required name never reported).\n"
        "Do not push an empty commit as the default.\n"
        "Recovery:\n"
        "1. Confirm this detector reports EXPECTED_STALE, not REQUIRED_ABSENT.\n"
        "2. Rerequest the check suites for this head: "
        "POST /repos/{owner}/{repo}/check-suites/{id}/rerequest\n"
        "3. If no suite exists, workflow_dispatch the producing workflow with "
        "the exact current head SHA.\n"
        "4. Push an empty commit only after (2) and (3) fail to re-register."
    ),
    "NOT_PROVEN": "Evidence is missing, contradictory, or insufficient.",
}


def required_names_from_policy(text: str) -> tuple[str, ...]:
    policy = tomllib.loads(text)
    names = [
        str(row["name"])
        for row in policy.get("checks", [])
        if isinstance(row, dict) and row.get("required") is True and row.get("name")
    ]
    return tuple(sorted(names))


def _as_text(value: Any) -> str:
    return value.strip() if isinstance(value, str) else ""


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
        "skipped": extra.pop("skipped", []),
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

    required = data.get("required_contexts")
    if not isinstance(required, list) or not required:
        return _not_proven("no required contexts supplied; refuse to invent a denominator")
    names: list[str] = []
    seen: set[str] = set()
    for item in required:
        name = _as_text(item)
        if not name:
            return _not_proven("required_contexts contains an empty name")
        if name in seen:
            return _not_proven(f"required context is duplicated: {name}")
        seen.add(name)
        names.append(name)

    raw_runs = data.get("check_runs")
    if not isinstance(raw_runs, list):
        return _not_proven("check_runs must be an array")

    runs_by_name: dict[str, list[dict[str, Any]]] = {name: [] for name in names}
    for run in raw_runs:
        if not isinstance(run, dict):
            return _not_proven("check_runs entries must be objects")
        name = _as_text(run.get("name"))
        if name in runs_by_name:
            runs_by_name[name].append(run)

    absent: list[str] = []
    other_sha: list[str] = []
    skipped: list[str] = []
    red: list[str] = []
    pending: list[str] = []
    rows: list[dict[str, Any]] = []

    for name in names:
        observed = runs_by_name[name]
        if not observed:
            absent.append(name)
            rows.append({"name": name, "state": "absent"})
            continue
        on_head = [
            run for run in observed
            if _normalize_sha(run.get("head_sha")) == head_sha
        ]
        if not on_head:
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

        terminal = [run for run in on_head if _status(run) == "completed"]
        if not terminal:
            pending.append(name)
            rows.append({"name": name, "state": "pending"})
            continue

        conclusions = {_conclusion(run) for run in terminal}
        if conclusions & RED:
            red.append(name)
            rows.append({"name": name, "state": "red"})
            continue
        if conclusions & SKIPPED and not (conclusions & SUCCESS):
            skipped.append(name)
            rows.append({"name": name, "state": "skipped"})
            continue
        if conclusions <= SUCCESS:
            rows.append({"name": name, "state": "success"})
            continue
        if any(not _status(run) or _status(run) in PENDING_STATUS for run in on_head):
            pending.append(name)
            rows.append({"name": name, "state": "pending"})
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
            "absent": absent,
            "other_sha": other_sha,
            "skipped": skipped,
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
            + f" Current head {head_sha[:12]}; observed {foreign[:12]}.",
            head_status="STALE_SUBJECT",
        )
    if skipped:
        return finish("REQUIRED_SKIPPED", head_status="SKIPPED")
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

    if _expected_checks_signal(evaluation):
        return finish("EXPECTED_STALE", head_status="HEAD_GREEN")

    merge_state = _as_text(evaluation.get("merge_state_status")).upper()
    if merge_state in {"CLEAN", "BEHIND", "UNSTABLE", ""} and mergeable in {"MERGEABLE", ""}:
        if merge_state == "UNSTABLE" and evaluation.get("blocked_by_expected_checks") is not False:
            return _not_proven(
                "merge_state_status is UNSTABLE without an expected-checks signal; do not infer a GitHub defect",
                head_status="HEAD_GREEN",
                required_contexts=names,
                rows=[],
            )
        return finish("COMPLETE_GREEN", head_status="HEAD_GREEN")
    if merge_state in {"BLOCKED", "UNSTABLE"}:
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
    data["required_contexts"] = list(required_names_from_policy(policy.read_text(encoding="utf-8")))
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
