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

Only complete canonical gates.v1 observations are published. Missing or invalid
timing is not zero cost, and a partial lane must not train the history estimator.
On refusal the exact output is invalidated; source receipts remain available.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import sys
import tempfile
import tomllib
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 1
GATES_SCHEMA_VERSION = "gates.v1"
SHARD_SUMMARY_SCHEMA_VERSION = "ci_gate_shard.v1"
MAX_DURATION_MS = 2**64 - 1


def read_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as f:
        return tomllib.load(f)


def receipt_inventory(receipts_dir: Path) -> tuple[list[Path], list[OSError]]:
    """Retain discovery errors and unsupported paths for input preservation."""
    paths: list[Path] = []
    errors: list[OSError] = []
    for directory, subdirectories, filenames in os.walk(receipts_dir, onerror=errors.append):
        for name in subdirectories:
            if (Path(directory) / name).is_symlink():
                paths.append(Path(directory) / name)
                errors.append(OSError(f"unsupported receipt directory symlink: {Path(directory) / name}"))
        paths.extend(Path(directory) / name for name in filenames if name.endswith(".json"))
    return sorted(paths), errors


def collect_receipts(receipts_dir: Path) -> list[dict[str, Any]]:
    """Load canonical envelopes, excluding the unrelated shard-summary family.

    Unknown/malformed evidence is not silently dropped: doing so would leave a
    plausible but incomplete lane sample. This does not deduplicate executions;
    equal paths or content are not a producer-bound execution identity.
    """
    if not receipts_dir.is_dir():
        raise ValueError(f"receipt directory is missing or not a directory: {receipts_dir}")
    paths, errors = receipt_inventory(receipts_dir)
    if errors:
        raise errors[0]
    out: list[dict[str, Any]] = []
    for path in paths:
        if not path.is_file():
            raise ValueError(f"receipt is not a regular file: {path}")
        try:
            doc = json.loads(path.read_text(encoding="utf-8"))
        except (ValueError, OSError) as error:
            raise ValueError(f"cannot read receipt {path}: {error}") from error
        if not isinstance(doc, dict):
            raise ValueError(f"receipt must be a canonical envelope: {path}")
        version = doc.get("schema_version")
        if version == SHARD_SUMMARY_SCHEMA_VERSION:
            continue
        if version != GATES_SCHEMA_VERSION:
            raise ValueError(f"unsupported receipt schema {version!r}: {path}")
        if not isinstance(doc.get("metadata"), dict) or not isinstance(doc.get("summary"), dict):
            raise ValueError(f"canonical receipt requires metadata and summary objects: {path}")
        gates = doc.get("gates")
        if not isinstance(gates, list) or not gates:
            raise ValueError(f"canonical receipt requires a nonempty gates array: {path}")
        total = doc["summary"].get("total_gates")
        if type(total) is not int or total != len(gates):
            raise ValueError(f"receipt gate population does not match summary.total_gates: {path}")
        for gate in gates:
            if not isinstance(gate, dict):
                raise ValueError(f"canonical gate must be an object: {path}")
            out.append({**gate, "_source_path": str(path)})
    if not out:
        raise ValueError(f"no canonical gate observations found in {receipts_dir}")
    return out


def finite_nonnegative(value: Any, field: str) -> float:
    """Validate cost inputs/results without accepting bool as a number."""
    if type(value) not in (int, float):
        raise ValueError(f"{field} must be a finite nonnegative number")
    try:
        number = float(value)
    except OverflowError as error:
        raise ValueError(f"{field} exceeds the finite numeric range") from error
    if not math.isfinite(number) or number < 0:
        raise ValueError(f"{field} must be a finite nonnegative number")
    return number


def lane_base_lem(lane_id: str, lanes: dict[str, Any]) -> float | None:
    lane = lanes.get(lane_id)
    if not isinstance(lane, dict):
        raise ValueError(f"lane {lane_id!r} must be a table")
    base = lane.get("base_lem")
    return None if base is None else finite_nonnegative(base, f"lane.{lane_id}.base_lem")


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
    if not receipts:
        raise ValueError("no canonical gate observations supplied")
    if not isinstance(multipliers, dict) or not isinstance(lanes, dict):
        raise ValueError("runner_multipliers and lane policy must be tables")
    if lane_id is not None and lane_id not in lanes:
        raise ValueError(f"--lane-id {lane_id!r} is not a known lane")
    jobs: list[dict[str, Any]] = []
    total_actual_lem = 0.0
    total_estimated_lem = 0.0
    # Lane floors are a property of the lane, not of each gate inside it. A
    # shard lane runs many gates, so adding its floor per job would multiply
    # the lane's whole budget by its gate count. Count each lane once.
    counted_lanes: set[str] = set()
    for r in receipts:
        if not isinstance(r, dict):
            raise ValueError("canonical gate must be an object")
        for field in ("gate_name", "tier", "status"):
            if not isinstance(r.get(field), str) or not r[field].strip():
                raise ValueError(f"canonical gate requires a nonempty {field} string")
        gate_name = r["gate_name"]
        duration_ms = r.get("duration_ms")
        if type(duration_ms) is not int or not 0 <= duration_ms <= MAX_DURATION_MS:
            raise ValueError(f"gate {gate_name!r} requires observed u64 duration_ms")
        runner = r.get("runner", runner_default)
        if not isinstance(runner, str) or not runner.strip():
            raise ValueError(f"gate {gate_name!r} requires a nonempty runner string")
        mult = finite_nonnegative(
            multipliers.get(runner, multipliers.get(runner_default, 1.0)),
            f"runner multiplier for {runner!r}",
        )
        actual_minutes = duration_ms / 1000.0 / 60.0
        actual_lem = finite_nonnegative(actual_minutes * mult, f"actual LEM for {gate_name!r}")
        total_actual_lem = finite_nonnegative(total_actual_lem + actual_lem, "total actual LEM")
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
                total_estimated_lem = finite_nonnegative(
                    total_estimated_lem + lane_floor, "total estimated LEM"
                )
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


def invalidate_output(args: argparse.Namespace) -> None:
    """Remove only the selected directory entry, never a policy/input path.

    Resolve parents to catch directory aliases, but do not follow the output's
    final symlink: unlinking it preserves the file it points to. Hardlink peers
    likewise survive the unlink and subsequent atomic replacement.
    """
    output = args.json_out.parent.resolve() / args.json_out.name
    if output.is_relative_to(args.receipts_dir.resolve()):
        raise ValueError("--json-out overlaps a receipt/policy input; refusing to remove input evidence")
    receipt_paths, discovery_errors = receipt_inventory(args.receipts_dir)
    for source in (args.budget, args.lanes, *receipt_paths):
        # Protect both a named input symlink and its referent. Input receipts
        # can point outside their root; deleting that referent destroys evidence.
        if output in (source.parent.resolve() / source.name, source.resolve()) or (
            source.is_dir() and output.is_relative_to(source.resolve())
        ):
            raise ValueError("--json-out overlaps a receipt/policy input; refusing to remove input evidence")
    args.json_out.unlink(missing_ok=True)
    if discovery_errors:
        raise discovery_errors[0]


def publish_actuals(output: Path, actuals: dict[str, Any]) -> None:
    """Publish complete strict JSON with a same-directory atomic replacement."""
    payload = json.dumps(actuals, indent=2, allow_nan=False) + "\n"
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=output.parent,
            prefix=f".{output.name}.", suffix=".tmp", delete=False,
        ) as stream:
            temporary = Path(stream.name)
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, output)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main() -> int:
    # Share the path definitions with error cleanup. A bad --pr or unknown
    # option must not leave yesterday's output available to an always-upload.
    path_defaults = {
        "receipts_dir": Path("target/receipts"),
        "budget": Path("policy/ci-budget.toml"),
        "lanes": Path("policy/ci-lanes.toml"),
        "json_out": Path("target/ci/ci-actuals.json"),
    }
    p = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    for name, default in path_defaults.items():
        p.add_argument("--" + name.replace("_", "-"), type=Path, default=default)
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
    try:
        args = p.parse_args()
    except SystemExit as error:
        if error.code:
            try:
                recovery = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
                for name in path_defaults:
                    recovery.add_argument(
                        "--" + name.replace("_", "-"), type=Path, nargs="?", action="append"
                    )
                recovered, _ = recovery.parse_known_args()
                known_paths = argparse.Namespace()
                for name, default in path_defaults.items():
                    values = getattr(recovered, name) or []
                    if name == "json_out" and values and values[-1] is None:
                        raise ValueError("--json-out has no value; no exact output can be identified")
                    value = next((v for v in reversed(values) if v is not None), default)
                    setattr(known_paths, name, value)
                invalidate_output(known_paths)
            except (argparse.ArgumentError, OSError, ValueError) as cleanup_error:
                print(f"::error::Cannot invalidate actuals output: {cleanup_error}", file=sys.stderr)
        return int(error.code or 0)

    try:
        invalidate_output(args)
        multipliers = read_toml(args.budget).get("runner_multipliers", {})
        lanes = read_toml(args.lanes).get("lane", {})
        actuals = emit_actuals(
            receipts=collect_receipts(args.receipts_dir),
            multipliers=multipliers,
            lanes=lanes,
            workflow=args.workflow,
            sha=args.sha,
            pr=args.pr,
            runner_default=args.runner_default,
            lane_id=args.lane_id,
        )
        publish_actuals(args.json_out, actuals)
    except (OSError, ValueError) as error:
        print(f"::error::CI actuals refused: {error}", file=sys.stderr)
        return 1

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
