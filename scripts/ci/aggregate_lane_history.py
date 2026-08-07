#!/usr/bin/env python3
"""Aggregate trusted ci-actuals artifacts into per-lane percentile history.

Inputs:
  --actuals-dir DIR      Walk DIR for ci-actuals*.json files.
  --window-days N        Only consider receipts newer than N days.
  --output PATH          Write the history JSON here.
  --static-lanes PATH    policy/ci-lanes.toml for the allowed lane set/floors.

Scheduled aggregation should additionally pass --require-trusted-markers,
--repository, and --default-branch. Each downloaded run directory must then
contain trusted-run.json written from verified GitHub workflow-run metadata.
"""
from __future__ import annotations

import argparse
from collections import Counter
import json
import math
import statistics
import sys
import time
import tomllib
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
MIN_SAMPLES_FOR_LEARNED = 5
MAX_ACTUAL_LEM = 10_000.0
MAX_JOBS_PER_RECEIPT = 100
MAX_SAMPLES_PER_LANE = 1_000
TRUSTED_MARKER = "trusted-run.json"


def percentile(values: list[float], p: float) -> float:
    """Linear-interpolation percentile, p in [0, 100]."""
    if not values:
        return 0.0
    if len(values) == 1:
        return values[0]
    sorted_vals = sorted(values)
    k = (len(values) - 1) * (p / 100.0)
    lo = int(math.floor(k))
    hi = int(math.ceil(k))
    if lo == hi:
        return sorted_vals[lo]
    return sorted_vals[lo] + (sorted_vals[hi] - sorted_vals[lo]) * (k - lo)


def static_floors(lanes_toml: Path) -> dict[str, float]:
    """Load and validate the canonical lane set and static floors."""
    if not lanes_toml.is_file():
        raise ValueError(f"lane policy not found: {lanes_toml}")

    try:
        doc = tomllib.loads(lanes_toml.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ValueError(f"could not read lane policy {lanes_toml}: {error}") from error

    lanes = doc.get("lane")
    if not isinstance(lanes, dict) or not lanes:
        raise ValueError("lane policy has no [lane.*] entries")

    out: dict[str, float] = {}
    for lane_id, lane in lanes.items():
        if not isinstance(lane_id, str) or not lane_id:
            raise ValueError(f"invalid lane id: {lane_id!r}")
        if not isinstance(lane, dict):
            raise ValueError(f"lane {lane_id!r} is not a table")
        base = lane.get("base_lem")
        if isinstance(base, bool) or not isinstance(base, (int, float)):
            raise ValueError(f"lane {lane_id!r} has non-numeric base_lem")
        floor = float(base)
        if not math.isfinite(floor) or floor < 0 or floor > MAX_ACTUAL_LEM:
            raise ValueError(f"lane {lane_id!r} has invalid base_lem {base!r}")
        out[lane_id] = floor
    return out


def find_trusted_marker(path: Path, actuals_dir: Path) -> Path | None:
    """Find the nearest run-level marker without escaping actuals_dir."""
    root = actuals_dir.resolve()
    current = path.parent.resolve()
    while True:
        marker = current / TRUSTED_MARKER
        if marker.is_file():
            return marker
        if current == root:
            return None
        if root not in current.parents:
            return None
        current = current.parent


def validate_trusted_marker(
    marker_path: Path,
    *,
    repository: str,
    default_branch: str,
) -> tuple[bool, str, int | None]:
    """Validate run provenance recorded by the trusted workflow shell."""
    try:
        marker = json.loads(marker_path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return False, "invalid_marker", None
    if not isinstance(marker, dict):
        return False, "invalid_marker", None

    run_id = marker.get("run_id")
    if isinstance(run_id, bool) or not isinstance(run_id, int) or run_id <= 0:
        return False, "invalid_marker", None
    if marker.get("repository") != repository:
        return False, "foreign_repository", run_id
    if marker.get("conclusion") != "success":
        return False, "unsuccessful_run", run_id

    event = marker.get("event")
    branch = marker.get("head_branch")
    if event == "push":
        if branch != default_branch:
            return False, "untrusted_branch", run_id
    elif event != "merge_group":
        return False, "untrusted_event", run_id

    head_sha = marker.get("head_sha")
    if not isinstance(head_sha, str) or len(head_sha) != 40:
        return False, "invalid_marker", run_id
    return True, "accepted", run_id


def collect_actuals(
    *,
    actuals_dir: Path,
    window_days: int,
    allowed_lanes: set[str],
    require_trusted_markers: bool = False,
    repository: str = "",
    default_branch: str = "",
) -> tuple[dict[str, list[float]], dict[str, Any]]:
    """Collect bounded samples and return validation/provenance statistics."""
    samples: dict[str, list[float]] = {}
    rejected: Counter[str] = Counter()
    source_runs: set[int] = set()
    stats: dict[str, Any] = {
        "files_seen": 0,
        "files_accepted": 0,
        "jobs_seen": 0,
        "accepted_samples": 0,
        "rejected": rejected,
        "source_run_ids": source_runs,
    }
    if not actuals_dir.exists():
        return samples, stats

    cutoff = time.time() - window_days * 86400
    for path in sorted(actuals_dir.rglob("ci-actuals*.json")):
        stats["files_seen"] += 1
        try:
            mtime = path.stat().st_mtime
        except OSError:
            rejected["unreadable_file"] += 1
            continue
        if mtime < cutoff:
            rejected["outside_window"] += 1
            continue

        if require_trusted_markers:
            marker_path = find_trusted_marker(path, actuals_dir)
            if marker_path is None:
                rejected["missing_marker"] += 1
                continue
            trusted, reason, run_id = validate_trusted_marker(
                marker_path,
                repository=repository,
                default_branch=default_branch,
            )
            if not trusted:
                rejected[reason] += 1
                continue
            if run_id is not None:
                source_runs.add(run_id)

        try:
            doc = json.loads(path.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError):
            rejected["invalid_receipt"] += 1
            continue
        if not isinstance(doc, dict):
            rejected["invalid_receipt"] += 1
            continue

        jobs = doc.get("jobs")
        if not isinstance(jobs, list):
            rejected["invalid_jobs"] += 1
            continue
        if len(jobs) > MAX_JOBS_PER_RECEIPT:
            rejected["oversized_receipt"] += 1
            continue

        stats["files_accepted"] += 1
        for job in jobs:
            stats["jobs_seen"] += 1
            if not isinstance(job, dict):
                rejected["invalid_job"] += 1
                continue

            lane_id = job.get("gate_name") or job.get("lane_id")
            if not isinstance(lane_id, str) or not lane_id:
                rejected["missing_lane"] += 1
                continue
            if lane_id not in allowed_lanes:
                rejected["unknown_lane"] += 1
                continue

            actual = job.get("actual_lem")
            if isinstance(actual, bool) or not isinstance(actual, (int, float)):
                rejected["invalid_actual"] += 1
                continue
            actual_value = float(actual)
            if not math.isfinite(actual_value):
                rejected["non_finite_actual"] += 1
                continue
            if actual_value < 0 or actual_value > MAX_ACTUAL_LEM:
                rejected["out_of_range_actual"] += 1
                continue

            lane_samples = samples.setdefault(lane_id, [])
            if len(lane_samples) >= MAX_SAMPLES_PER_LANE:
                rejected["lane_sample_cap"] += 1
                continue
            lane_samples.append(actual_value)
            stats["accepted_samples"] += 1

    return samples, stats


def serializable_stats(stats: dict[str, Any]) -> dict[str, Any]:
    """Convert counters/sets to deterministic JSON values."""
    return {
        "files_seen": int(stats["files_seen"]),
        "files_accepted": int(stats["files_accepted"]),
        "jobs_seen": int(stats["jobs_seen"]),
        "accepted_samples": int(stats["accepted_samples"]),
        "rejected": dict(sorted(stats["rejected"].items())),
        "source_run_count": len(stats["source_run_ids"]),
        "source_run_ids": sorted(stats["source_run_ids"]),
        "limits": {
            "max_actual_lem": MAX_ACTUAL_LEM,
            "max_jobs_per_receipt": MAX_JOBS_PER_RECEIPT,
            "max_samples_per_lane": MAX_SAMPLES_PER_LANE,
        },
    }


def build_history(
    *,
    samples: dict[str, list[float]],
    floors: dict[str, float],
    window_days: int,
    validation: dict[str, Any] | None = None,
) -> dict[str, Any]:
    lanes_out: dict[str, Any] = {}
    for lane_id in sorted(floors):
        lane_samples = samples.get(lane_id, [])
        entry: dict[str, Any] = {
            "samples": len(lane_samples),
            "static_floor": floors[lane_id],
            "learned": len(lane_samples) >= MIN_SAMPLES_FOR_LEARNED,
        }
        if lane_samples:
            entry.update(
                {
                    "p50": percentile(lane_samples, 50),
                    "p90": percentile(lane_samples, 90),
                    "p95": percentile(lane_samples, 95),
                    "min": min(lane_samples),
                    "max": max(lane_samples),
                    "mean": statistics.fmean(lane_samples),
                }
            )
        lanes_out[lane_id] = entry

    history: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "window_days": window_days,
        "min_samples_for_learned": MIN_SAMPLES_FOR_LEARNED,
        "lane_count": len(lanes_out),
        "lanes": lanes_out,
    }
    if validation is not None:
        history["validation"] = validation
    return history


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--actuals-dir",
        type=Path,
        default=Path("target/ci/actuals"),
        help="Directory to walk for ci-actuals artifacts.",
    )
    parser.add_argument("--window-days", type=int, default=14)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(".ci/metrics/ci-lane-history.json"),
    )
    parser.add_argument(
        "--static-lanes",
        type=Path,
        default=Path("policy/ci-lanes.toml"),
    )
    parser.add_argument("--require-trusted-markers", action="store_true")
    parser.add_argument("--repository", default="")
    parser.add_argument("--default-branch", default="")
    args = parser.parse_args()

    if args.window_days <= 0:
        parser.error("--window-days must be positive")
    if args.require_trusted_markers and (
        not args.repository or not args.default_branch
    ):
        parser.error(
            "--repository and --default-branch are required with "
            "--require-trusted-markers"
        )

    try:
        floors = static_floors(args.static_lanes)
    except ValueError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    samples, raw_stats = collect_actuals(
        actuals_dir=args.actuals_dir,
        window_days=args.window_days,
        allowed_lanes=set(floors),
        require_trusted_markers=args.require_trusted_markers,
        repository=args.repository,
        default_branch=args.default_branch,
    )
    validation = serializable_stats(raw_stats)
    history = build_history(
        samples=samples,
        floors=floors,
        window_days=args.window_days,
        validation=validation,
    )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(history, indent=2, allow_nan=False) + "\n",
        encoding="utf-8",
    )

    learned = sum(1 for entry in history["lanes"].values() if entry["learned"])
    print(
        json.dumps(
            {
                "lanes": history["lane_count"],
                "learned": learned,
                "window_days": args.window_days,
                "accepted_samples": validation["accepted_samples"],
                "rejected_samples": sum(validation["rejected"].values()),
                "source_runs": validation["source_run_count"],
            },
            allow_nan=False,
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
