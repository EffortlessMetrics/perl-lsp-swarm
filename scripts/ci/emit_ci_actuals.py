#!/usr/bin/env python3
"""Emit ci-actuals.json from gate receipts produced by xtask.

Reads:
  target/receipts/**/*.json   (or path passed via --receipts-dir)
  policy/ci-budget.toml       (for runner multipliers)
  policy/ci-lanes.toml        (for static base_lem floor)

Writes:
  target/ci/ci-actuals.json   (or path passed via --json-out)

This is the actuals counterpart to ci-plan.json. The plan forecasts cost; the
actuals records what was actually spent. Together they let later PRs derive
learned LEM estimates (PR 16 in the rollout).

Tolerant by design: receipts may have missing/partial fields, especially
during the rollout window. Missing duration_ms results in a null actual_lem;
the receipt is still recorded.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import tomllib
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 1


def read_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as f:
        return tomllib.load(f)


def collect_receipts(receipts_dir: Path) -> list[dict[str, Any]]:
    """Walk receipts_dir for *.json files, parse, and return as list."""
    if not receipts_dir.exists():
        return []
    out: list[dict[str, Any]] = []
    for path in sorted(receipts_dir.rglob("*.json")):
        try:
            doc = json.loads(path.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError):
            continue
        # Receipts have either a top-level `gates` array (gate-policy receipts)
        # or are a single gate receipt themselves.
        if isinstance(doc, dict) and isinstance(doc.get("gates"), list):
            for gate in doc["gates"]:
                if isinstance(gate, dict):
                    gate.setdefault("_source_path", str(path))
                    out.append(gate)
        elif isinstance(doc, dict) and "gate_name" in doc:
            doc.setdefault("_source_path", str(path))
            out.append(doc)
    return out


def lane_base_lem(lane_id: str, lanes: dict[str, Any]) -> float | None:
    lane = lanes.get(lane_id)
    if not isinstance(lane, dict):
        return None
    base = lane.get("base_lem")
    if isinstance(base, (int, float)):
        return float(base)
    return None


def emit_actuals(
    *,
    receipts: list[dict[str, Any]],
    multipliers: dict[str, float],
    lanes: dict[str, Any],
    workflow: str,
    sha: str,
    pr: int | None,
    runner_default: str,
    lane_id: str | None = None,
) -> dict[str, Any]:
    jobs: list[dict[str, Any]] = []
    total_actual_lem = 0.0
    total_estimated_lem = 0.0
    # Lane floors are a property of the lane, not of each gate inside it. A
    # shard lane runs many gates, so adding its floor per job would multiply
    # the lane's whole budget by its gate count. Count each lane once.
    counted_lanes: set[str] = set()
    for r in receipts:
        gate_name = r.get("gate_name", "")
        duration_ms = r.get("duration_ms")
        runner = r.get("runner") or runner_default
        mult = float(multipliers.get(runner, multipliers.get(runner_default, 1.0)))
        if isinstance(duration_ms, (int, float)) and duration_ms > 0:
            actual_minutes = float(duration_ms) / 1000.0 / 60.0
            actual_lem = actual_minutes * mult
            total_actual_lem += actual_lem
        else:
            actual_minutes = None
            actual_lem = None
        # Attribute against the static floor of the lane this job ran in.
        #
        # The lane is supplied by the workflow via --lane-id, because the
        # workflow job is the only place that knows it: gate names are N:1
        # into lanes (`fmt`, `clippy_full`, and `unit_foundation_full` all run
        # inside one shard lane), so the mapping cannot be recovered from the
        # gate name. It must not be guessed either — `compile_all_targets` and
        # `check_all_targets`, `docs_build` and `docs_gate` are near-miss pairs
        # that any fuzzy match would bind to the wrong lane (#6217).
        #
        # A bare gate name is honoured only when it is *literally* a known lane
        # id, which is an exact lookup rather than a match heuristic.
        resolved_lane = lane_id or (gate_name if gate_name in lanes else None)

        # Per-job `estimated_lem` stays the job's *own* static estimate, which
        # exists only when the gate is itself a whole lane (1:1). A gate that
        # is one of many inside a lane has no floor of its own, so it reports
        # null rather than borrowing the lane's.
        estimated_lem = (
            lane_base_lem(gate_name, lanes) if gate_name in lanes else None
        )
        if resolved_lane is not None and resolved_lane not in counted_lanes:
            counted_lanes.add(resolved_lane)
            lane_floor = lane_base_lem(resolved_lane, lanes)
            if lane_floor is not None:
                total_estimated_lem += lane_floor
        jobs.append(
            {
                "lane_id": resolved_lane,
                "gate_name": gate_name,
                "tier": r.get("tier"),
                "status": r.get("status"),
                "runner": runner,
                "duration_ms": duration_ms,
                "actual_minutes": actual_minutes,
                "actual_lem": actual_lem,
                "estimated_lem": estimated_lem,
                "source_path": r.get("_source_path"),
            }
        )

    return {
        "schema_version": SCHEMA_VERSION,
        "repo": "perl-lsp",
        "sha": sha,
        "pr": pr,
        "workflow": workflow,
        "totals": {
            "actual_lem": total_actual_lem,
            "estimated_lem": total_estimated_lem,
            "delta_lem": total_actual_lem - total_estimated_lem,
        },
        "jobs": jobs,
    }


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--receipts-dir", type=Path, default=Path("target/receipts"))
    p.add_argument("--budget", type=Path, default=Path("policy/ci-budget.toml"))
    p.add_argument("--lanes", type=Path, default=Path("policy/ci-lanes.toml"))
    p.add_argument(
        "--json-out", type=Path, default=Path("target/ci/ci-actuals.json")
    )
    p.add_argument("--workflow", default=os.environ.get("GITHUB_WORKFLOW", ""))
    p.add_argument("--sha", default=os.environ.get("GITHUB_SHA", "HEAD"))
    p.add_argument("--pr", type=int, default=None)
    p.add_argument(
        "--runner-default",
        default="ubuntu_24_04",
        help="Runner to assume when receipts don't carry a runner field.",
    )
    p.add_argument(
        "--lane-id",
        default=None,
        help=(
            "Lane id from policy/ci-lanes.toml that this job ran in, e.g. "
            "merge_gate_shards. Stamped onto every emitted job so the "
            "aggregator can attribute samples to a real lane. Gate names are "
            "N:1 into lanes and cannot be mapped back, so the invoking "
            "workflow must supply this (#6217)."
        ),
    )
    args = p.parse_args()

    multipliers = read_toml(args.budget).get("runner_multipliers", {})
    lanes = read_toml(args.lanes).get("lane", {})

    if args.lane_id is not None and args.lane_id not in lanes:
        print(
            f"::error::--lane-id {args.lane_id!r} is not a lane in {args.lanes}. "
            "Samples stamped with an unknown lane are dropped by the "
            "aggregator, so this is refused rather than emitted (#6217).",
            file=sys.stderr,
        )
        return 1

    receipts = collect_receipts(args.receipts_dir)
    actuals = emit_actuals(
        receipts=receipts,
        multipliers=multipliers,
        lanes=lanes,
        workflow=args.workflow,
        sha=args.sha,
        pr=args.pr,
        runner_default=args.runner_default,
        lane_id=args.lane_id,
    )

    args.json_out.parent.mkdir(parents=True, exist_ok=True)
    args.json_out.write_text(json.dumps(actuals, indent=2) + "\n", encoding="utf-8")

    print(
        json.dumps(
            {
                "jobs": len(actuals["jobs"]),
                "actual_lem": actuals["totals"]["actual_lem"],
                "estimated_lem": actuals["totals"]["estimated_lem"],
            }
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
