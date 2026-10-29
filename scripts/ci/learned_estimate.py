#!/usr/bin/env python3
"""Read a lane's learned LEM estimate from .ci/metrics/ci-lane-history.json.

Used by the PR Plan workflow once ci-actuals.json artifacts have accumulated
enough samples per lane (>= MIN_SAMPLES_FOR_LEARNED in
aggregate_lane_history.py).

Estimate model:
  estimate = max(static_floor, p50_recent_actual * 1.15)
  warning  = p90_recent_actual
  hard_planning = p95_recent_actual

If the history is missing or the lane has too few samples, fall back to the
static floor and report `learned: false`.

Output is JSON on stdout so the caller can pipe into jq. Every stdout object
carries `schema_version` (`learned_estimate.v1`) so a later consumer can
refuse an unfamiliar shape. That token is this producer's field; it is not
the history file's integer `schema_version`.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

SCHEMA_VERSION = "learned_estimate.v1"


def emit_stdout(payload: dict[str, Any]) -> None:
    """Print one stdout JSON object. Always stamps this producer's schema token.

    A payload key named `schema_version` cannot override the producer field:
    that name is also used by the history file for a different integer contract.
    """
    body = {key: value for key, value in payload.items() if key != "schema_version"}
    print(json.dumps({"schema_version": SCHEMA_VERSION, **body}))

_HERE = Path(__file__).resolve().parent
if str(_HERE) not in sys.path:
    sys.path.insert(0, str(_HERE))

# The history file is a versioned wire contract written by
# `aggregate_lane_history.py`. This gate pins the version *this reader* can
# parse, not the producer's current `aggregate_lane_history.SCHEMA_VERSION`:
# importing the producer constant would advance the check automatically when
# the producer bumps and renames a field, silently accepting a shape the v1
# field names below cannot read (#15286). Move this constant only together
# with the parser below. (Named SUPPORTED_*, not SCHEMA_VERSION: that name is
# this producer's stdout token, a different string contract.)
SUPPORTED_SCHEMA_VERSION = 1


def history_version_refusal(history: Any) -> str | None:
    """Why this payload cannot be read as a v1 lane history, or `None`.

    The reader below resolves `lanes`, `p50` and `static_floor` by name, so a
    payload written by any other version is not merely unrecognised -- it is
    read with the wrong field names. A renamed `static_floor` makes
    `estimate_for`'s floor guard silently fall through and report a bare
    `p50 * 1.15`, dropping a static floor that exists to prevent exactly that
    optimism. Refusing is the only honest answer to a shape this reader does
    not know.
    """
    if not isinstance(history, dict):
        return f"lane history payload is not a JSON object, got {type(history).__name__}"
    found = history.get("schema_version")
    # bool is an int subclass and 1.0 == 1: bare `!=` would admit JSON `true`
    # and `1.0` as v1 (same pitfall `pr_plan.load_learned_history` guards).
    # An exact int is required; anything else is an unfamiliar shape.
    if type(found) is not int or found != SUPPORTED_SCHEMA_VERSION:
        return (
            f"lane history schema_version must be {SUPPORTED_SCHEMA_VERSION}, "
            f"got {found!r}; refusing to read an unfamiliar shape"
        )
    return None


def estimate_for(lane_id: str, history: dict[str, Any]) -> dict[str, Any]:
    lane = (history.get("lanes") or {}).get(lane_id)
    if not isinstance(lane, dict):
        return {
            "lane": lane_id,
            "learned": False,
            "estimate": None,
            "static_floor": None,
            "samples": 0,
            "reason": "no history entry for this lane",
        }
    floor = lane.get("static_floor")
    if not lane.get("learned"):
        return {
            "lane": lane_id,
            "learned": False,
            "estimate": floor,
            "static_floor": floor,
            "samples": lane.get("samples", 0),
            "reason": (
                f"only {lane.get('samples', 0)} samples; need "
                f"{history.get('min_samples_for_learned', 5)} to learn"
            ),
        }
    p50 = lane.get("p50")
    p90 = lane.get("p90")
    p95 = lane.get("p95")
    if p50 is None:
        return {
            "lane": lane_id,
            "learned": False,
            "estimate": floor,
            "static_floor": floor,
            "samples": lane.get("samples", 0),
            "reason": "history missing p50",
        }
    learned_estimate = p50 * 1.15
    if floor is not None and floor > learned_estimate:
        # Static floor is higher than learned p50*1.15; respect the floor.
        # This guards against runaway optimism when p50 lags real cost.
        chosen = floor
        chosen_source = "static_floor (higher than learned)"
    else:
        chosen = learned_estimate
        chosen_source = "p50 * 1.15"
    return {
        "lane": lane_id,
        "learned": True,
        "estimate": chosen,
        "estimate_source": chosen_source,
        "static_floor": floor,
        "p50": p50,
        "p90_warning": p90,
        "p95_hard_planning": p95,
        "samples": lane.get("samples", 0),
    }


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--history",
        type=Path,
        default=Path(".ci/metrics/ci-lane-history.json"),
    )
    p.add_argument("--lane", required=True, help="Lane id to query.")
    args = p.parse_args()

    if not args.history.exists():
        emit_stdout(
            {
                "lane": args.lane,
                "learned": False,
                "estimate": None,
                "samples": 0,
                "reason": f"history file {args.history} not present",
            }
        )
        return 0

    try:
        history = json.loads(args.history.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError) as e:
        emit_stdout({"lane": args.lane, "learned": False, "error": str(e)})
        return 0

    if not isinstance(history, dict):
        emit_stdout(
            {
                "lane": args.lane,
                "learned": False,
                "estimate": None,
                "samples": 0,
                "reason": (
                    "history payload is not a JSON object, "
                    f"got {type(history).__name__}"
                ),
            }
        )
        return 0

    refusal = history_version_refusal(history)
    if refusal is not None:
        emit_stdout(
            {
                "lane": args.lane,
                "learned": False,
                "estimate": None,
                "samples": 0,
                "reason": refusal,
            }
        )
        return 0

    emit_stdout(estimate_for(args.lane, history))
    return 0


if __name__ == "__main__":
    sys.exit(main())
