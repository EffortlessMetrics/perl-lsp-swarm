#!/usr/bin/env python3
"""Classify exact-SHA CI shard evidence for the Rust Small result gate.

The caller supplies check-run payloads for the current ``main`` SHA and the
current PR/merge-group subject SHA. Only a completed, failure-like main shard
paired with candidate evidence that is not completed green can block. Missing
or stale main evidence is reported as a warning and is non-blocking; missing
or non-green candidate evidence must not allow a merge through a recorded main
red.
"""

from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

SHARD_NAMES = (
    "CI Gate shard (meta)",
    "CI Gate shard (foundation)",
    "CI Gate shard (parser_stack)",
    "CI Gate shard (analysis)",
    "CI Gate shard (lsp)",
    "CI Gate shard (support)",
    "CI Gate shard (corpus)",
    "CI Gate shard (policy)",
)
GITHUB_ACTIONS_APP_ID = 15368
RED_CONCLUSIONS = frozenset(
    {"failure", "timed_out", "startup_failure", "action_required"}
)
CANDIDATE_RETRY_CONCLUSIONS = frozenset(
    {"cancelled", "skipped"}
)
RUN_ID_PATTERN = re.compile(r"/actions/runs/([0-9]+)(?:/|$)")


@dataclass
class Decision:
    blockers: list[str] = field(default_factory=list)
    waiters: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)

    @property
    def blocks(self) -> bool:
        return bool(self.blockers)

    @property
    def waits_for_candidate(self) -> bool:
        return bool(self.waiters) and not self.blockers


def _flatten_payload(payload: Any) -> list[dict[str, Any]]:
    """Accept both one-page and ``gh api --paginate --slurp`` payloads."""
    pages = payload if isinstance(payload, list) else [payload]
    runs: list[dict[str, Any]] = []
    for page in pages:
        if isinstance(page, dict):
            candidates = page.get("check_runs", [])
        else:
            candidates = page
        if isinstance(candidates, list):
            runs.extend(run for run in candidates if isinstance(run, dict))
    return runs


def load_payload(path: Path) -> tuple[list[dict[str, Any]], str | None]:
    """Return check runs, or a warning string for an unusable API response."""
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        return [], f"could not read check-run probe {path.name}: {error}"
    runs = _flatten_payload(payload)
    if not runs:
        return [], f"check-run probe {path.name} contained no check runs"
    return runs, None


def workflow_run_ids(payload: Any, subject_sha: str) -> set[int]:
    """Return canonical ``ci.yml`` run IDs for one exact subject SHA."""
    pages = payload if isinstance(payload, list) else [payload]
    run_ids: set[int] = set()
    for page in pages:
        candidates = page.get("workflow_runs", []) if isinstance(page, dict) else []
        if not isinstance(candidates, list):
            continue
        for run in candidates:
            if (
                isinstance(run, dict)
                and run.get("head_sha") == subject_sha
                and isinstance(run.get("id"), int)
            ):
                run_ids.add(run["id"])
    return run_ids


def load_workflow_run_ids(path: Path, subject_sha: str) -> tuple[set[int], str | None]:
    """Load canonical workflow-run IDs, preserving probe failures for callers."""
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        return set(), f"could not read workflow-run probe {path.name}: {error}"
    run_ids = workflow_run_ids(payload, subject_sha)
    if not run_ids:
        return set(), f"workflow-run probe {path.name} contained no exact-SHA ci.yml run"
    return run_ids, None


def _run_sort_key(run: dict[str, Any]) -> tuple[str, int]:
    # A newer in-progress rerun must not be hidden by an older completed run.
    timestamp = str(
        run.get("started_at")
        or run.get("run_started_at")
        or run.get("created_at")
        or ""
    )
    run_id = run.get("id")
    return timestamp, run_id if isinstance(run_id, int) else 0


def _latest_shards(
    runs: list[dict[str, Any]],
    subject_sha: str,
    canonical_run_ids: set[int] | None = None,
) -> dict[str, dict[str, Any]]:
    selected: dict[str, dict[str, Any]] = {}
    for run in runs:
        name = run.get("name")
        app = run.get("app")
        details_url = str(run.get("details_url") or "")
        run_match = RUN_ID_PATTERN.search(details_url)
        if (
            name not in SHARD_NAMES
            or run.get("head_sha") != subject_sha
            or not isinstance(app, dict)
            or app.get("id") != GITHUB_ACTIONS_APP_ID
            or (
                canonical_run_ids is not None
                and (run_match is None or int(run_match.group(1)) not in canonical_run_ids)
            )
        ):
            continue
        previous = selected.get(name)
        if previous is None or _run_sort_key(run) > _run_sort_key(previous):
            selected[name] = run
    return selected


def _conclusion(run: dict[str, Any]) -> str:
    return str(run.get("conclusion") or "").strip().lower()


def _describe_probe_state(run: dict[str, Any]) -> str:
    status = str(run.get("status") or "missing").strip().lower()
    conclusion = _conclusion(run) or "none"
    return f"status={status}, conclusion={conclusion}"


def evaluate(
    *,
    main_runs: list[dict[str, Any]],
    candidate_runs: list[dict[str, Any]],
    main_sha_before: str,
    main_sha_after: str,
    candidate_sha: str,
    main_probe_warning: str | None = None,
    candidate_probe_warning: str | None = None,
    main_workflow_run_ids: set[int] | None = None,
    candidate_workflow_run_ids: set[int] | None = None,
    main_workflow_probe_warning: str | None = None,
    candidate_workflow_probe_warning: str | None = None,
    main_workflow_sha: str = "",
    candidate_workflow_sha: str = "",
) -> Decision:
    """Apply the fail-closed refusal policy to already-fetched API data."""
    decision = Decision()
    if main_probe_warning:
        decision.warnings.append(main_probe_warning)
    if candidate_probe_warning:
        decision.warnings.append(candidate_probe_warning)
    if main_workflow_probe_warning:
        decision.warnings.append(main_workflow_probe_warning)
    if candidate_workflow_probe_warning:
        decision.warnings.append(candidate_workflow_probe_warning)
    if main_probe_warning or main_workflow_probe_warning or not main_workflow_run_ids:
        decision.warnings.append(
            "main-red refusal main evidence is incomplete; treating probe data as non-blocking"
        )
        return decision
    if not main_sha_before or not main_sha_after:
        decision.warnings.append("main SHA could not be read before and after the probe; refusing to block")
        return decision
    if not main_workflow_sha:
        decision.warnings.append("canonical main ci.yml could not be read; refusing to block")
        return decision
    if main_sha_before != main_sha_after:
        decision.warnings.append(
            f"main moved during the probe ({main_sha_before} -> {main_sha_after}); probe is stale and non-blocking"
        )
        return decision
    if not candidate_sha:
        decision.warnings.append("no PR or merge-group subject SHA is available; refusal probe is non-applicable")
        return decision

    main_by_name = _latest_shards(main_runs, main_sha_before, main_workflow_run_ids)
    if candidate_workflow_sha != main_workflow_sha:
        decision.warnings.append(
            "candidate ci.yml differs from canonical main ci.yml; candidate shard evidence is not comparable"
        )
        candidate_workflow_run_ids = set()
    candidate_by_name = _latest_shards(
        candidate_runs,
        candidate_sha,
        candidate_workflow_run_ids,
    )
    for name in SHARD_NAMES:
        main = main_by_name.get(name)
        candidate = candidate_by_name.get(name)
        if main is None:
            decision.warnings.append(f"{name}: no exact-SHA main check run")
            continue
        if str(main.get("status") or "").lower() != "completed":
            decision.warnings.append(
                f"{name}: main probe is not completed ({_describe_probe_state(main)})"
            )
            continue
        main_conclusion = _conclusion(main)
        if main_conclusion not in RED_CONCLUSIONS:
            if main_conclusion not in {"success"}:
                decision.warnings.append(
                    f"{name}: main probe is not a recorded red result ({_describe_probe_state(main)})"
                )
            continue
        if candidate is None:
            decision.waiters.append(
                f"{name}: main {main_conclusion} and no exact-SHA completed candidate result"
            )
            continue
        if str(candidate.get("status") or "").lower() != "completed":
            decision.waiters.append(
                f"{name}: main {main_conclusion} and candidate is not completed "
                f"({_describe_probe_state(candidate)})"
            )
            continue

        candidate_conclusion = _conclusion(candidate)
        if candidate_conclusion == "success":
            continue
        if candidate_conclusion in RED_CONCLUSIONS:
            decision.blockers.append(
                f"{name}: main {main_conclusion} and candidate {candidate_conclusion}"
            )
            continue
        if candidate_conclusion in CANDIDATE_RETRY_CONCLUSIONS:
            decision.waiters.append(
                f"{name}: main {main_conclusion} and candidate must be retried "
                f"({_describe_probe_state(candidate)})"
            )
            continue
        decision.blockers.append(
            f"{name}: main {main_conclusion} and candidate is not green "
            f"({_describe_probe_state(candidate)})"
        )
    return decision


def finalize(decision: Decision) -> Decision:
    """Fail closed when the bounded candidate-evidence wait is exhausted."""
    if decision.waiters:
        decision.blockers.extend(decision.waiters)
        decision.waiters.clear()
    return decision


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--main-runs", type=Path, required=True)
    parser.add_argument("--candidate-runs", type=Path, required=True)
    parser.add_argument("--main-workflow-runs", type=Path, required=True)
    parser.add_argument("--candidate-workflow-runs", type=Path, required=True)
    parser.add_argument("--main-sha-before", required=True)
    parser.add_argument("--main-sha-after", required=True)
    parser.add_argument("--candidate-sha", required=True)
    parser.add_argument("--main-workflow-sha", required=True)
    parser.add_argument("--candidate-workflow-sha", required=True)
    parser.add_argument(
        "--final",
        action="store_true",
        help="Convert candidate evidence waits into blockers after the bounded poll.",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    main_runs, main_warning = load_payload(args.main_runs)
    candidate_runs, candidate_warning = load_payload(args.candidate_runs)
    main_workflow_ids, main_workflow_warning = load_workflow_run_ids(
        args.main_workflow_runs,
        args.main_sha_before,
    )
    candidate_workflow_ids, candidate_workflow_warning = load_workflow_run_ids(
        args.candidate_workflow_runs,
        args.candidate_sha,
    )
    decision = evaluate(
        main_runs=main_runs,
        candidate_runs=candidate_runs,
        main_sha_before=args.main_sha_before,
        main_sha_after=args.main_sha_after,
        candidate_sha=args.candidate_sha,
        main_probe_warning=main_warning,
        candidate_probe_warning=candidate_warning,
        main_workflow_run_ids=main_workflow_ids,
        candidate_workflow_run_ids=candidate_workflow_ids,
        main_workflow_probe_warning=main_workflow_warning,
        candidate_workflow_probe_warning=candidate_workflow_warning,
        main_workflow_sha=args.main_workflow_sha,
        candidate_workflow_sha=args.candidate_workflow_sha,
    )
    if args.final:
        finalize(decision)
    for warning in decision.warnings:
        print(f"::warning::{warning}")
    for blocker in decision.blockers:
        print(f"::error::main-red refusal: {blocker}")
    if decision.blockers:
        print("Perl LSP Rust Small Result: blocked by recorded main-red shard refusal")
        return 1
    if decision.waits_for_candidate:
        for waiter in decision.waiters:
            print(f"::notice::main-red refusal waiting for candidate evidence: {waiter}")
        print("Perl LSP Rust Small Result: waiting for candidate shard evidence")
        return 2
    print("Perl LSP Rust Small Result: main-red refusal probe non-blocking")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
