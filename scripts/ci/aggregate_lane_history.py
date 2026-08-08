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
from datetime import datetime, timezone
import json
import math
from pathlib import Path
import re
import statistics
import sys
import tempfile
import time
import tomllib
from typing import Any

SCHEMA_VERSION = 1
MIN_SAMPLES_FOR_LEARNED = 5
# actual_lem is wall-clock minutes multiplied by the runner multiplier. The
# policy's maximum multiplier is 10x, so 600 LEM bounds a one-hour run while
# rejecting finite artifact values that could dominate learned percentiles.
MAX_ACTUAL_LEM = 600.0
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
    k = (len(sorted_vals) - 1) * (p / 100.0)
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
    except (OSError, ValueError) as error:
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
    if current != root and root not in current.parents:
        return None

    while True:
        marker = current / TRUSTED_MARKER
        if marker.is_file():
            return marker
        if current == root:
            return None
        current = current.parent


def parse_trusted_timestamp(value: Any) -> float | None:
    """Parse one timezone-aware RFC 3339 timestamp from trusted run metadata."""
    if not isinstance(value, str) or not value:
        return None
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return None
    return parsed.timestamp()


def validate_trusted_marker(
    marker_path: Path,
    *,
    repository: str,
    default_branch: str,
) -> tuple[bool, str, int | None, float | None]:
    """Validate run provenance recorded by the trusted workflow shell."""
    try:
        marker = json.loads(marker_path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return False, "invalid_marker", None, None
    if not isinstance(marker, dict):
        return False, "invalid_marker", None, None

    run_id = marker.get("run_id")
    if isinstance(run_id, bool) or not isinstance(run_id, int) or run_id <= 0:
        return False, "invalid_marker", None, None
    if marker.get("repository") != repository:
        return False, "foreign_repository", run_id, None
    if marker.get("conclusion") != "success":
        return False, "unsuccessful_run", run_id, None

    event = marker.get("event")
    branch = marker.get("head_branch")
    if event == "push":
        if branch != default_branch:
            return False, "untrusted_branch", run_id, None
    elif event != "merge_group":
        return False, "untrusted_event", run_id, None

    head_sha = marker.get("head_sha")
    if not isinstance(head_sha, str) or re.fullmatch(r"[0-9a-f]{40}", head_sha) is None:
        return False, "invalid_marker", run_id, None

    created_at = parse_trusted_timestamp(marker.get("created_at"))
    if created_at is None:
        return False, "invalid_marker", run_id, None
    return True, "accepted", run_id, created_at


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

        if require_trusted_markers:
            marker_path = find_trusted_marker(path, actuals_dir)
            if marker_path is None:
                rejected["missing_marker"] += 1
                continue
            trusted, reason, run_id, created_at = validate_trusted_marker(
                marker_path,
                repository=repository,
                default_branch=default_branch,
            )
            if not trusted:
                rejected[reason] += 1
                continue
            if created_at is None or created_at < cutoff:
                rejected["outside_window"] += 1
                continue
            if run_id is not None:
                source_runs.add(run_id)
        else:
            try:
                mtime = path.stat().st_mtime
            except OSError:
                rejected["unreadable_file"] += 1
                continue
            if mtime < cutoff:
                rejected["outside_window"] += 1
                continue

        try:
            doc = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
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
            try:
                actual_value = float(actual)
            except (OverflowError, ValueError):
                rejected["invalid_actual"] += 1
                continue
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


def self_test_write_run(
    root: Path,
    *,
    run_id: int = 123,
    event: str = "push",
    branch: str = "main",
    repository: str = "EffortlessMetrics/perl-lsp-swarm",
    conclusion: str = "success",
    created_at: str | None = None,
    jobs: list[object] | None = None,
    write_marker: bool = True,
) -> None:
    """Write one synthetic downloaded run for the built-in security tests."""
    run_dir = root / f"run-{run_id}"
    artifact_dir = run_dir / "ci-actuals-meta"
    artifact_dir.mkdir(parents=True)
    if write_marker:
        marker_created_at = created_at or datetime.now(timezone.utc).strftime(
            "%Y-%m-%dT%H:%M:%SZ"
        )
        (run_dir / TRUSTED_MARKER).write_text(
            json.dumps(
                {
                    "run_id": run_id,
                    "repository": repository,
                    "event": event,
                    "head_branch": branch,
                    "head_sha": "a" * 40,
                    "conclusion": conclusion,
                    "created_at": marker_created_at,
                }
            )
            + "\n",
            encoding="utf-8",
        )
    (artifact_dir / "ci-actuals-meta.json").write_text(
        json.dumps({"jobs": jobs or []}) + "\n",
        encoding="utf-8",
    )


def run_self_tests() -> None:
    """Exercise provenance, lane, value, and resource rejection contracts."""
    repository = "EffortlessMetrics/perl-lsp-swarm"
    default_branch = "main"

    def check(condition: bool, message: str) -> None:
        if not condition:
            raise AssertionError(message)

    def collect(root: Path) -> tuple[dict[str, list[float]], dict[str, Any]]:
        return collect_actuals(
            actuals_dir=root,
            window_days=14,
            allowed_lanes={"meta"},
            require_trusted_markers=True,
            repository=repository,
            default_branch=default_branch,
        )

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        self_test_write_run(
            root,
            jobs=[{"gate_name": "meta", "actual_lem": 42.5}],
        )
        samples, raw_stats = collect(root)
        stats = serializable_stats(raw_stats)
        check(samples == {"meta": [42.5]}, "trusted default-branch push rejected")
        check(stats["source_run_ids"] == [123], "source run identity not recorded")
        check(stats["accepted_samples"] == 1, "accepted sample count incorrect")

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        self_test_write_run(
            root,
            event="merge_group",
            branch="gh-readonly-queue/main/pr-1-deadbeef",
            jobs=[{"lane_id": "meta", "actual_lem": 7}],
        )
        samples, _ = collect(root)
        check(samples == {"meta": [7.0]}, "trusted merge-group sample rejected")

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        self_test_write_run(
            root,
            run_id=3,
            repository="attacker/fork",
            jobs=[{"lane_id": "meta", "actual_lem": 3}],
        )
        self_test_write_run(
            root,
            run_id=4,
            conclusion="failure",
            jobs=[{"lane_id": "meta", "actual_lem": 4}],
        )
        self_test_write_run(
            root,
            run_id=5,
            branch="feature",
            jobs=[{"lane_id": "meta", "actual_lem": 5}],
        )
        samples, raw_stats = collect(root)
        rejected = serializable_stats(raw_stats)["rejected"]
        check(samples == {}, "untrusted provenance supplied a sample")
        check(rejected.get("foreign_repository") == 1, "foreign repo not rejected")
        check(rejected.get("unsuccessful_run") == 1, "failed run not rejected")
        check(rejected.get("untrusted_branch") == 1, "non-default branch not rejected")

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        self_test_write_run(
            root,
            run_id=1,
            event="pull_request",
            branch="feature",
            jobs=[{"lane_id": "meta", "actual_lem": 1}],
        )
        self_test_write_run(
            root,
            run_id=2,
            jobs=[{"lane_id": "meta", "actual_lem": 2}],
            write_marker=False,
        )
        samples, raw_stats = collect(root)
        rejected = serializable_stats(raw_stats)["rejected"]
        check(samples == {}, "untrusted run supplied a sample")
        check(rejected.get("untrusted_event") == 1, "PR source was not rejected")
        check(rejected.get("missing_marker") == 1, "missing marker was not rejected")

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        self_test_write_run(
            root,
            run_id=6,
            created_at="2000-01-01T00:00:00Z",
            jobs=[{"lane_id": "meta", "actual_lem": 6}],
        )
        samples, raw_stats = collect(root)
        rejected = serializable_stats(raw_stats)["rejected"]
        check(samples == {}, "out-of-window trusted run supplied a sample")
        check(rejected.get("outside_window") == 1, "marker timestamp was not enforced")

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        self_test_write_run(
            root,
            jobs=[
                {"lane_id": "unknown", "actual_lem": 1},
                {"lane_id": "meta", "actual_lem": True},
                {"lane_id": "meta", "actual_lem": "1"},
                {"lane_id": "meta", "actual_lem": math.nan},
                {"lane_id": "meta", "actual_lem": -1},
                {"lane_id": "meta", "actual_lem": MAX_ACTUAL_LEM + 1},
                {"lane_id": "meta", "actual_lem": 10**400},
                {"lane_id": "meta", "actual_lem": 3},
            ],
        )
        samples, raw_stats = collect(root)
        rejected = serializable_stats(raw_stats)["rejected"]
        check(samples == {"meta": [3.0]}, "valid sample was not isolated")
        check(rejected.get("unknown_lane") == 1, "unknown lane was not rejected")
        check(rejected.get("invalid_actual") == 3, "invalid actual count incorrect")
        check(rejected.get("non_finite_actual") == 1, "NaN was not rejected")
        check(rejected.get("out_of_range_actual") == 2, "range checks failed")

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        self_test_write_run(
            root,
            jobs=[
                {"lane_id": "meta", "actual_lem": index}
                for index in range(MAX_JOBS_PER_RECEIPT + 1)
            ],
        )
        samples, raw_stats = collect(root)
        rejected = serializable_stats(raw_stats)["rejected"]
        check(samples == {}, "oversized receipt supplied samples")
        check(rejected.get("oversized_receipt") == 1, "oversized receipt not rejected")

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        for run_id in range(11):
            self_test_write_run(
                root,
                run_id=100 + run_id,
                jobs=[
                    {"lane_id": "meta", "actual_lem": 1}
                    for _ in range(100)
                ],
            )
        samples, raw_stats = collect(root)
        rejected = serializable_stats(raw_stats)["rejected"]
        check(
            len(samples["meta"]) == MAX_SAMPLES_PER_LANE,
            "per-lane sample cap was not enforced",
        )
        check(
            rejected.get("lane_sample_cap") == 100,
            "per-lane overflow samples were not rejected",
        )

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        valid = root / "valid.toml"
        valid.write_text('[lane.meta]\nbase_lem = 2.5\n', encoding="utf-8")
        check(static_floors(valid) == {"meta": 2.5}, "valid lane policy rejected")

        empty = root / "empty.toml"
        empty.write_text("", encoding="utf-8")
        try:
            static_floors(empty)
        except ValueError:
            pass
        else:
            raise AssertionError("empty lane policy did not fail closed")

        invalid = root / "invalid.toml"
        invalid.write_text('[lane.meta]\nbase_lem = "large"\n', encoding="utf-8")
        try:
            static_floors(invalid)
        except ValueError:
            pass
        else:
            raise AssertionError("non-numeric lane floor did not fail closed")

    print("aggregate_lane_history self-test passed")


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
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        run_self_tests()
        return 0
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
