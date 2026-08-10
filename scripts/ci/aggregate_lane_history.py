#!/usr/bin/env python3
"""Aggregate ci-actuals.json artifacts into a per-lane percentile history.

Inputs (one or more):
  --actuals-dir DIR      Walk DIR for ci-actuals.json files (default: target/ci/actuals/).
  --window-days N        Only consider receipts newer than N days (default: 14).
  --output PATH          Write history JSON here (default: .ci/metrics/ci-lane-history.json).
  --static-lanes PATH    policy/ci-lanes.toml for static_floor reference.

Output: .ci/metrics/ci-lane-history.json with per-lane sample counts and
p50/p90/p95. PR 13's planner can consume this to derive learned estimates
once enough samples accumulate.

Schema is intentionally tolerant: lanes with fewer than MIN_SAMPLES
samples are still recorded but flagged with `learned: false` so the
planner falls back to the static floor.
"""
from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
import time
import tomllib
from datetime import date
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
MIN_SAMPLES_FOR_LEARNED = 5

# Artifacts emitted before `--lane-id` existed carry no `lane_id`, and the
# aggregator reads a 14-day window, so for a couple of weeks after the #6217
# wiring lands there will legitimately be nothing to attribute. That is a
# mechanical, self-resolving fact and it warns rather than fails.
#
# It expires. A workflow that never got the `--lane-id` wiring must not sit
# warning forever, because a permanent warning is the same silence this change
# exists to remove, just arrived at more slowly. After this date, "no artifact
# carries lane_id" is itself an error.
LANE_ID_ROLLOUT_DEADLINE = date(2026, 9, 1)


def percentile(values: list[float], p: float) -> float:
    """Linear-interpolation percentile, p in [0, 100]."""
    if not values:
        return 0.0
    if len(values) == 1:
        return values[0]
    sorted_vals = sorted(values)
    k = (len(sorted_vals) - 1) * (p / 100.0)
    lo = int(math.floor(k))
    hi = int(math.ceil(k))
    if lo == hi:
        return sorted_vals[lo]
    return sorted_vals[lo] + (sorted_vals[hi] - sorted_vals[lo]) * (k - lo)


def collect_actuals(
    *, actuals_dir: Path, window_days: int, known_lanes: set[str]
) -> tuple[dict[str, list[float]], dict[str, Any]]:
    """Walk actuals_dir for ci-actuals.json files, return per-lane LEM samples.

    Only samples that attribute to a lane in ``known_lanes`` are accepted. A
    job is attributed by its explicit ``lane_id`` (stamped by the emitter from
    the workflow's own ``--lane-id``), or by ``gate_name`` when that name is
    *literally* a known lane id.

    A ``gate_name`` that is not a lane id is counted as unmapped and dropped.
    It is deliberately not minted into a new lane: gate names are N:1 into
    lanes, so doing that builds a parallel keyspace no planner can read while
    every real lane stays empty, which is the #6217 defect.

    **One sample per lane execution, not per gate.** A single
    ``merge_gate_shards`` run spans eight matrix jobs and dozens of gates, all
    stamped with the same lane. Appending each gate's ``actual_lem``
    separately would let one workflow run clear the five-sample threshold on
    its own, and would make p50/p90/p95 describe a typical *gate* rather than
    the lane's total cost. ``pr_plan.py`` consumes those percentiles as
    whole-lane actuals, so per-gate samples would sit permanently below the
    lane's static floor and the lane could never calibrate *upward* — which is
    the entire purpose of a learned estimate. Jobs are therefore grouped by
    (run, workflow, lane) across shard artifacts and summed into one sample.

    Returns ``(samples, stats)``; ``stats`` is the validation record the caller
    uses to decide whether the run learned anything at all.
    """
    samples: dict[str, list[float]] = {}
    stats: dict[str, Any] = {
        "source_files": 0,
        "jobs_seen": 0,
        "jobs_with_sample": 0,
        # Jobs carrying a `lane_id` field at all, whether or not it names a
        # known lane. Zero means every artifact in the window predates the
        # emitter's --lane-id wiring, which is the rollout state rather than a
        # mapping failure.
        "jobs_with_lane_id": 0,
        "accepted_samples": 0,
        # Distinct (run, workflow, lane) groups, i.e. how many samples actually
        # land in the history. Always <= accepted_samples, and much smaller for
        # a shard lane.
        "lane_executions": 0,
        "unmapped_samples": 0,
        "unmapped_keys": {},
    }
    # (run, workflow, lane) -> summed LEM for that one lane execution.
    executions: dict[tuple[str, str, str], float] = {}
    if not actuals_dir.exists():
        return samples, stats

    cutoff = time.time() - window_days * 86400
    for path in sorted(actuals_dir.rglob("*.json")):
        try:
            mtime = path.stat().st_mtime
        except OSError:
            continue
        if mtime < cutoff:
            continue
        try:
            doc = json.loads(path.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError):
            continue
        if not isinstance(doc, dict):
            continue
        stats["source_files"] += 1
        # Identity of the workflow run this artifact came from. The eight shard
        # artifacts of one run share a sha, which is what lets them sum into a
        # single lane execution. Falling back to the artifact's own directory
        # keeps unrelated runs apart when sha is absent, which over-counts
        # rather than merging distinct runs into one inflated sample.
        run_key = str(doc.get("sha") or path.parent)
        workflow_key = str(doc.get("workflow") or "")
        for job in doc.get("jobs", []):
            if not isinstance(job, dict):
                continue
            stats["jobs_seen"] += 1
            actual = job.get("actual_lem")
            if not isinstance(actual, (int, float)):
                continue
            # Reject non-finite or extreme samples that could corrupt the
            # percentile history (inf, nan, or implausibly large values from
            # a buggy or malicious ci-actuals artifact) (#5995).
            actual_float = float(actual)
            if not math.isfinite(actual_float) or actual_float < 0 or actual_float > 3_600_000:
                continue
            stats["jobs_with_sample"] += 1

            # Explicit lane_id wins. A gate_name counts only on an exact match
            # against a known lane id — never a prefix, suffix, or fuzzy match
            # (#6217).
            gate_name = job.get("gate_name")
            lane_id = job.get("lane_id")
            if lane_id:
                stats["jobs_with_lane_id"] += 1
            if lane_id not in known_lanes:
                lane_id = gate_name if gate_name in known_lanes else None

            if lane_id is None:
                stats["unmapped_samples"] += 1
                key = gate_name or job.get("lane_id") or "<unnamed>"
                stats["unmapped_keys"][key] = stats["unmapped_keys"].get(key, 0) + 1
                continue

            stats["accepted_samples"] += 1
            key = (run_key, workflow_key, lane_id)
            executions[key] = executions.get(key, 0.0) + actual_float

    # One sample per lane execution, summed across that run's gates and shards.
    for (_run, _workflow, lane_id), total in executions.items():
        samples.setdefault(lane_id, []).append(total)
    stats["lane_executions"] = len(executions)
    return samples, stats


def static_floors(lanes_toml: Path) -> dict[str, float]:
    if not lanes_toml.exists():
        return {}
    doc = tomllib.loads(lanes_toml.read_text(encoding="utf-8"))
    out: dict[str, float] = {}
    for lane_id, lane in doc.get("lane", {}).items():
        base = lane.get("base_lem")
        if isinstance(base, (int, float)):
            out[lane_id] = float(base)
    return out


def build_history(
    *,
    samples: dict[str, list[float]],
    floors: dict[str, float],
    window_days: int,
    stats: dict[str, Any] | None = None,
) -> dict[str, Any]:
    lanes_out: dict[str, Any] = {}
    # Include every lane known to policy, even when samples are empty: planner
    # readers can iterate the full keyspace without checking presence.
    for lane_id in sorted(set(samples.keys()) | set(floors.keys())):
        s = samples.get(lane_id, [])
        floor = floors.get(lane_id)
        learned = len(s) >= MIN_SAMPLES_FOR_LEARNED
        entry: dict[str, Any] = {
            "samples": len(s),
            "static_floor": floor,
            "learned": learned,
        }
        if s:
            entry.update(
                {
                    "p50": percentile(s, 50),
                    "p90": percentile(s, 90),
                    "p95": percentile(s, 95),
                    "min": min(s),
                    "max": max(s),
                    "mean": statistics.fmean(s),
                }
            )
        lanes_out[lane_id] = entry

    out: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "window_days": window_days,
        "min_samples_for_learned": MIN_SAMPLES_FOR_LEARNED,
        "lane_count": len(lanes_out),
        "lanes": lanes_out,
    }
    if stats is not None:
        # Recorded so a reader can tell "no data arrived" apart from "data
        # arrived and none of it attributed to a lane" — the two states this
        # file could not previously distinguish (#6217).
        out["validation"] = dict(stats)
    return out


def attribution_verdict(
    stats: dict[str, Any],
    *,
    today: date,
    deadline: date = LANE_ID_ROLLOUT_DEADLINE,
) -> tuple[int, str | None]:
    """Decide whether this run's attribution is acceptable.

    Three outcomes, deliberately distinguished (#6217):

    - **quiet ok** — nothing arrived, or samples attributed to real lanes.
      Nothing was claimed and nothing was lost.
    - **warn** — samples arrived, none attributed, and *no artifact in the
      window carries a lane_id at all*. Every artifact predates the emitter
      wiring, which is mechanical and self-resolving. Expires at ``deadline``.
    - **error** — samples arrived, none attributed, and artifacts *do* carry
      lane_ids. The wiring exists and is still producing nothing usable, which
      is the real defect and fails from day one.

    Returns ``(exit_code, message)``.
    """
    if stats["jobs_with_sample"] == 0:
        return 0, None
    if stats["accepted_samples"] > 0:
        return 0, None

    worst = sorted(stats["unmapped_keys"].items(), key=lambda kv: -kv[1])[:10]
    detail = ", ".join(f"{name}={count}" for name, count in worst)
    preamble = (
        f"Lane-history aggregation attributed 0 of {stats['jobs_with_sample']} "
        "samples to a known lane, so every lane in the written history is "
        f"empty. Most frequent unmapped keys: {detail}."
    )

    if stats["jobs_with_lane_id"] == 0 and today < deadline:
        return 0, (
            f"::warning::{preamble} No artifact in the window carries a "
            "lane_id yet, so this is the expected #6217 rollout window while "
            "pre-wiring artifacts age out. This becomes an ERROR on "
            f"{deadline.isoformat()}; if it is still warning then, the "
            "emitting workflow never got its --lane-id and must be fixed."
        )

    if stats["jobs_with_lane_id"] == 0:
        return 1, (
            f"::error::{preamble} No artifact carries a lane_id and the "
            f"rollout window closed on {deadline.isoformat()}: the emitting "
            "workflow is not passing --lane-id (#6217)."
        )

    return 1, (
        f"::error::{preamble} Artifacts do carry lane_ids "
        f"({stats['jobs_with_lane_id']} of {stats['jobs_with_sample']} "
        "sampled jobs), so this is not the rollout window: those lane_ids do "
        "not name lanes in policy/ci-lanes.toml (#6217)."
    )


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--actuals-dir", type=Path, default=Path("target/ci/actuals"),
        help="Directory to walk for ci-actuals.json artifacts.",
    )
    p.add_argument("--window-days", type=int, default=14)
    p.add_argument(
        "--output", type=Path,
        default=Path(".ci/metrics/ci-lane-history.json"),
    )
    p.add_argument(
        "--static-lanes", type=Path, default=Path("policy/ci-lanes.toml")
    )
    args = p.parse_args()

    floors = static_floors(args.static_lanes)
    samples, stats = collect_actuals(
        actuals_dir=args.actuals_dir,
        window_days=args.window_days,
        known_lanes=set(floors),
    )
    history = build_history(
        samples=samples,
        floors=floors,
        window_days=args.window_days,
        stats=stats,
    )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(history, indent=2) + "\n", encoding="utf-8"
    )

    learned = sum(1 for entry in history["lanes"].values() if entry["learned"])
    print(
        json.dumps(
            {
                "lanes": history["lane_count"],
                "learned": learned,
                "window_days": args.window_days,
                "accepted_samples": stats["accepted_samples"],
                "unmapped_samples": stats["unmapped_samples"],
            }
        )
    )

    # Loudness, discriminated. A run that attributed nothing is either the
    # mechanical rollout window (warn, expiring) or a live mapping failure
    # (error from day one). Collapsing the two would either hide the real
    # defect or emit a chronic red that everyone learns to ignore — the same
    # ignored-signal failure as #6188, #6193, #6202, and #6229.
    code, message = attribution_verdict(stats, today=date.today())
    if message is not None:
        print(message, file=sys.stderr)
    return code


if __name__ == "__main__":
    sys.exit(main())
