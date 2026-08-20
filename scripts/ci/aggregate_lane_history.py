#!/usr/bin/env python3
"""Aggregate trusted ci-actuals artifacts into per-lane percentile history.

Inputs:
  --actuals-dir DIR      Walk DIR for ci-actuals*.json files.
  --window-days N        Only consider receipts newer than N days.
  --output PATH          Write the history JSON here.
  --static-lanes PATH    policy/ci-lanes.toml for the allowed lane set/floors.

Scheduled aggregation should additionally pass --require-trusted-markers,
--repository, and --default-branch. Each downloaded run directory must then
contain a run-level trusted-run.json written from verified GitHub workflow-run
metadata. Downloaded artifacts cannot provide or override that marker.
"""
from __future__ import annotations

import argparse
from collections import Counter
from dataclasses import dataclass
from datetime import datetime
import json
import math
from pathlib import Path
import re
import stat
import statistics
import sys
import time
import tomllib
from datetime import date, datetime
from typing import Any

SCHEMA_VERSION = 1
EXPECTED_RECEIPT_SCHEMA_VERSION = 1
EXPECTED_RECEIPT_REPO = "perl-lsp"
MIN_SAMPLES_FOR_LEARNED = 5
MAX_ACTUAL_LEM = 125.0
MAX_RECEIPT_BYTES = 1_048_576
MAX_MARKER_BYTES = 16_384
MAX_JOBS_PER_RECEIPT = 100
MAX_SAMPLES_PER_LANE = 1_000
TRUSTED_MARKER = "trusted-run.json"
RUN_DIRECTORY_RE = re.compile(r"run-([1-9][0-9]*)")
FULL_SHA_RE = re.compile(r"[0-9a-f]{40}")


@dataclass(frozen=True)
class TrustedRun:
    """Validated provenance for one downloaded workflow run."""

    run_id: int
    head_sha: str
    created_at: float

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


def static_floors(lanes_toml: Path) -> dict[str, float]:
    """Load and validate the canonical lane allowlist and static floors."""
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


def bounded_regular_file(path: Path, *, max_bytes: int) -> tuple[bool, str]:
    """Check that path is one bounded ordinary file without following symlinks."""
    try:
        metadata = path.lstat()
    except OSError:
        return False, "unreadable_file"
    if not stat.S_ISREG(metadata.st_mode):
        return False, "non_regular_file"
    if metadata.st_size > max_bytes:
        return False, "oversized_file"
    return True, "accepted"


def trusted_marker_for_receipt(
    receipt_path: Path, actuals_dir: Path
) -> tuple[Path, int] | None:
    """Derive the exact run-level marker; never search inside artifact content."""
    try:
        root = actuals_dir.resolve(strict=True)
        receipt = receipt_path.resolve(strict=True)
        relative = receipt.relative_to(root)
    except (OSError, ValueError):
        return None

    # The workflow extracts only below:
    #   <actuals_dir>/run-<id>/artifacts/**/ci-actuals*.json
    # The trusted marker is written afterward at:
    #   <actuals_dir>/run-<id>/trusted-run.json
    if len(relative.parts) < 3 or relative.parts[1] != "artifacts":
        return None
    run_match = RUN_DIRECTORY_RE.fullmatch(relative.parts[0])
    if run_match is None:
        return None
    run_dir = root / relative.parts[0]
    try:
        run_metadata = run_dir.lstat()
    except OSError:
        return None
    if not stat.S_ISDIR(run_metadata.st_mode):
        return None
    return run_dir / TRUSTED_MARKER, int(run_match.group(1))


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
    expected_run_id: int,
    repository: str,
    default_branch: str,
) -> tuple[TrustedRun | None, str]:
    """Validate run provenance recorded by the trusted workflow shell."""
    regular, reason = bounded_regular_file(marker_path, max_bytes=MAX_MARKER_BYTES)
    if not regular:
        return None, f"marker_{reason}"
    try:
        marker = json.loads(marker_path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None, "invalid_marker"
    if not isinstance(marker, dict):
        return None, "invalid_marker"

    run_id = marker.get("run_id")
    if (
        isinstance(run_id, bool)
        or not isinstance(run_id, int)
        or run_id != expected_run_id
    ):
        return None, "run_id_mismatch"
    if marker.get("repository") != repository:
        return None, "foreign_repository"
    if marker.get("conclusion") != "success":
        return None, "unsuccessful_run"

    event = marker.get("event")
    branch = marker.get("head_branch")
    if event == "push":
        if branch != default_branch:
            return None, "untrusted_branch"
    elif event != "merge_group":
        return None, "untrusted_event"

    head_sha = marker.get("head_sha")
    if not isinstance(head_sha, str) or FULL_SHA_RE.fullmatch(head_sha) is None:
        return None, "invalid_marker"

    created_at = parse_trusted_timestamp(marker.get("created_at"))
    if created_at is None:
        return None, "invalid_marker"
    return TrustedRun(run_id=run_id, head_sha=head_sha, created_at=created_at), "accepted"


def validate_receipt_identity(
    doc: dict[str, Any], *, trusted_run: TrustedRun | None, expected_repo: str
) -> str | None:
    """Return a rejection reason when receipt identity is missing or contradictory."""
    if doc.get("schema_version") != EXPECTED_RECEIPT_SCHEMA_VERSION:
        return "unsupported_receipt_schema"
    if doc.get("repo") != expected_repo:
        return "receipt_repo_mismatch"
    workflow = doc.get("workflow")
    if not isinstance(workflow, str) or not workflow.strip():
        return "missing_workflow_identity"
    pr_number = doc.get("pr")
    if isinstance(pr_number, bool) or pr_number not in (None, 0):
        return "pull_request_receipt"
    sha = doc.get("sha")
    if not isinstance(sha, str) or FULL_SHA_RE.fullmatch(sha) is None:
        return "invalid_receipt_sha"
    if trusted_run is not None and sha != trusted_run.head_sha:
        return "receipt_sha_mismatch"
    return None


def collect_actuals(
    *,
    actuals_dir: Path,
    window_days: int,
    allowed_lanes: set[str],
    require_trusted_markers: bool = False,
    repository: str = "",
    default_branch: str = "",
    expected_receipt_repo: str = EXPECTED_RECEIPT_REPO,
) -> tuple[dict[str, list[float]], dict[str, Any]]:
    """Collect bounded samples and return validation/provenance statistics."""
    samples: dict[str, list[float]] = {}
    rejected: Counter[str] = Counter()
    source_runs: set[int] = set()
    executions: dict[tuple[str, str, str], float] = {}
    stats: dict[str, Any] = {
        "files_seen": 0,
        "files_accepted": 0,
        "jobs_seen": 0,
        "jobs_with_sample": 0,
        "jobs_with_lane_id": 0,
        "accepted_samples": 0,
        "lane_executions": 0,
        "unmapped_samples": 0,
        "unmapped_keys": {},
        "rejected": rejected,
        "source_run_ids": source_runs,
    }
    if not actuals_dir.exists():
        return samples, stats
    if not actuals_dir.is_dir() or actuals_dir.is_symlink():
        raise ValueError(f"actuals directory is not a regular directory: {actuals_dir}")

    cutoff = time.time() - window_days * 86400
    for path in sorted(actuals_dir.rglob("ci-actuals*.json")):
        stats["files_seen"] += 1
        regular, file_reason = bounded_regular_file(path, max_bytes=MAX_RECEIPT_BYTES)
        if not regular:
            rejected[
                "oversized_receipt"
                if file_reason == "oversized_file"
                else "non_regular_receipt"
            ] += 1
            continue

        trusted_run: TrustedRun | None = None
        if require_trusted_markers:
            marker = trusted_marker_for_receipt(path, actuals_dir)
            if marker is None:
                rejected["missing_marker"] += 1
                continue
            marker_path, expected_run_id = marker
            if not marker_path.exists():
                rejected["missing_marker"] += 1
                continue
            trusted_run, marker_reason = validate_trusted_marker(
                marker_path,
                expected_run_id=expected_run_id,
                repository=repository,
                default_branch=default_branch,
            )
            if trusted_run is None:
                rejected[marker_reason] += 1
                continue
            if trusted_run.created_at < cutoff:
                rejected["outside_window"] += 1
                continue
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

        if require_trusted_markers:
            identity_reason = validate_receipt_identity(
                doc,
                trusted_run=trusted_run,
                expected_repo=expected_receipt_repo,
            )
            if identity_reason is not None:
                rejected[identity_reason] += 1
                continue

        jobs = doc.get("jobs")
        if not isinstance(jobs, list):
            rejected["invalid_jobs"] += 1
            continue
        if len(jobs) > MAX_JOBS_PER_RECEIPT:
            rejected["oversized_receipt"] += 1
            continue

        stats["files_accepted"] += 1
        if trusted_run is not None:
            source_runs.add(trusted_run.run_id)
            run_key = str(trusted_run.run_id)
        else:
            run_key = str(doc.get("sha") or path.parent)
        workflow_key = str(doc.get("workflow") or "")
        for job in jobs:
            stats["jobs_seen"] += 1
            if not isinstance(job, dict):
                rejected["invalid_job"] += 1
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
            stats["jobs_with_sample"] += 1

            gate_name = job.get("gate_name")
            lane_id = job.get("lane_id")
            if lane_id:
                stats["jobs_with_lane_id"] += 1
            if lane_id not in allowed_lanes:
                lane_id = gate_name if gate_name in allowed_lanes else None

            if lane_id is None:
                stats["unmapped_samples"] += 1
                key = gate_name or job.get("lane_id") or "<unnamed>"
                stats["unmapped_keys"][key] = stats["unmapped_keys"].get(key, 0) + 1
                continue

            stats["accepted_samples"] += 1
            key = (run_key, workflow_key, lane_id)
            executions[key] = executions.get(key, 0.0) + actual_value

    for (_run, _workflow, lane_id), total in executions.items():
        lane_samples = samples.setdefault(lane_id, [])
        if len(lane_samples) >= MAX_SAMPLES_PER_LANE:
            rejected["lane_sample_cap"] += 1
            continue
        lane_samples.append(total)
    stats["lane_executions"] = len(executions)
    return samples, stats


def serializable_stats(stats: dict[str, Any]) -> dict[str, Any]:
    """Convert counters/sets to deterministic JSON values."""
    return {
        "files_seen": int(stats["files_seen"]),
        "files_accepted": int(stats["files_accepted"]),
        "jobs_seen": int(stats["jobs_seen"]),
        "accepted_samples": int(stats["accepted_samples"]),
        "jobs_with_sample": int(stats["jobs_with_sample"]),
        "jobs_with_lane_id": int(stats["jobs_with_lane_id"]),
        "lane_executions": int(stats["lane_executions"]),
        "unmapped_samples": int(stats["unmapped_samples"]),
        "unmapped_keys": dict(stats["unmapped_keys"]),
        "rejected": dict(sorted(stats["rejected"].items())),
        "source_run_count": len(stats["source_run_ids"]),
        "source_run_ids": sorted(stats["source_run_ids"]),
        "limits": {
            "max_actual_lem": MAX_ACTUAL_LEM,
            "max_receipt_bytes": MAX_RECEIPT_BYTES,
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
    """Build deterministic per-lane statistics from validated samples."""
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


def validate_history_payload(data: Any) -> list[str]:
    """Structurally validate a lane-history payload; return violations.

    Independent oracle over the checked-in artifact (#11731): the producer's
    own smoke check runs in the same context that generated the file, so the
    repository also needs a reader-side gate that can go red on a payload
    defect — percentile ordering, sample/learned agreement, counter
    coherence, and lane-count identity are all asserted here.
    """
    violations: list[str] = []

    def finite(value: Any) -> bool:
        return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)

    if not isinstance(data, dict):
        return ["payload is not a JSON object"]
    if data.get("schema_version") != SCHEMA_VERSION:
        violations.append(f"schema_version must be {SCHEMA_VERSION}, got {data.get('schema_version')!r}")
    if not isinstance(data.get("generated_at"), str) or not data.get("generated_at"):
        violations.append("generated_at must be a non-empty string")
    min_samples = data.get("min_samples_for_learned")
    if not isinstance(min_samples, int) or isinstance(min_samples, bool) or min_samples < 1:
        violations.append(f"min_samples_for_learned must be a positive int, got {min_samples!r}")

    lanes = data.get("lanes")
    if not isinstance(lanes, dict) or not lanes:
        violations.append("lanes must be a non-empty object")
        lanes = {}
    if data.get("lane_count") != len(lanes):
        violations.append(f"lane_count {data.get('lane_count')!r} != len(lanes) {len(lanes)}")

    total_samples = 0
    for lane_id, lane in lanes.items():
        where = f"lane {lane_id!r}"
        if not isinstance(lane, dict):
            violations.append(f"{where} is not an object")
            continue
        samples = lane.get("samples")
        if not isinstance(samples, int) or isinstance(samples, bool) or samples < 0:
            violations.append(f"{where} samples must be a non-negative int, got {samples!r}")
            samples = 0
        total_samples += samples
        floor = lane.get("static_floor")
        if not finite(floor) or floor < 0:
            violations.append(f"{where} static_floor must be finite and >= 0, got {floor!r}")
        learned = lane.get("learned")
        if not isinstance(learned, bool):
            violations.append(f"{where} learned must be a bool, got {learned!r}")
        elif isinstance(min_samples, int) and learned != (samples >= min_samples):
            violations.append(
                f"{where} learned={learned} disagrees with samples={samples} "
                f"vs min_samples_for_learned={min_samples}"
            )

        percentile_keys = ("p50", "p90", "p95", "min", "max", "mean")
        has_stats = any(k in lane for k in percentile_keys)
        if samples == 0 and has_stats:
            violations.append(f"{where} has percentile fields but samples == 0")
        if samples > 0:
            if not has_stats:
                violations.append(f"{where} has samples={samples} but no percentile fields")
            else:
                values = {k: lane.get(k) for k in percentile_keys}
                for k, v in values.items():
                    if not finite(v):
                        violations.append(f"{where} {k} must be finite, got {v!r}")
                if all(finite(v) for v in values.values()):
                    if not values["min"] <= values["p50"]:
                        violations.append(f"{where} min > p50")
                    if not values["p50"] <= values["p90"]:
                        violations.append(f"{where} p50 > p90")
                    if not values["p90"] <= values["p95"]:
                        violations.append(f"{where} p90 > p95")
                    if not values["p95"] <= values["max"]:
                        violations.append(f"{where} p95 > max")
                    if not values["min"] <= values["mean"] <= values["max"]:
                        violations.append(f"{where} mean outside [min, max]")

    validation = data.get("validation")
    if not isinstance(validation, dict):
        violations.append("validation summary must be an object")
    else:
        run_ids = validation.get("source_run_ids")
        if not isinstance(run_ids, list) or not all(isinstance(r, int) for r in run_ids):
            violations.append("validation.source_run_ids must be a list of ints")
            run_ids = []
        declared = validation.get("source_run_count")
        if declared != len(run_ids):
            violations.append(f"validation.source_run_count {declared!r} != len(source_run_ids) {len(run_ids)}")
        for counter in ("files_seen", "files_accepted", "jobs_seen", "jobs_with_sample",
                        "jobs_with_lane_id", "accepted_samples", "lane_executions"):
            value = validation.get(counter)
            if not isinstance(value, int) or isinstance(value, bool) or value < 0:
                violations.append(f"validation.{counter} must be a non-negative int, got {value!r}")
        if isinstance(validation.get("files_seen"), int) and isinstance(validation.get("files_accepted"), int):
            if validation["files_accepted"] > validation["files_seen"]:
                violations.append("validation.files_accepted > files_seen")
        if isinstance(validation.get("jobs_with_sample"), int) and isinstance(validation.get("jobs_seen"), int):
            if validation["jobs_with_sample"] > validation["jobs_seen"]:
                violations.append("validation.jobs_with_sample > jobs_seen")
        if isinstance(validation.get("lane_executions"), int):
            if validation["lane_executions"] != total_samples:
                violations.append(
                    f"validation.lane_executions {validation['lane_executions']} != sum of lane samples {total_samples}"
                )

    return violations


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
    if stats["lane_executions"] > 0:
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
    """CLI entry point."""
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
    parser.add_argument("--receipt-repo", default=EXPECTED_RECEIPT_REPO)
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
        samples, raw_stats = collect_actuals(
            actuals_dir=args.actuals_dir,
            window_days=args.window_days,
            allowed_lanes=set(floors),
            require_trusted_markers=args.require_trusted_markers,
            repository=args.repository,
            default_branch=args.default_branch,
            expected_receipt_repo=args.receipt_repo,
        )
    except ValueError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

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
                "unmapped_samples": validation["unmapped_samples"],
            },
            allow_nan=False,
        )
    )

    # Loudness, discriminated. A run that attributed nothing is either the
    # mechanical rollout window (warn, expiring) or a live mapping failure
    # (error from day one). Collapsing the two would either hide the real
    # defect or emit a chronic red that everyone learns to ignore — the same
    # ignored-signal failure as #6188, #6193, #6202, and #6229.
    code, message = attribution_verdict(raw_stats, today=date.today())
    if message is not None:
        print(message, file=sys.stderr)
    return code


if __name__ == "__main__":
    sys.exit(main())
