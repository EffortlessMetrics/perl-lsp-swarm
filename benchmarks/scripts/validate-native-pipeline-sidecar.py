#!/usr/bin/env python3
"""Fail-closed validation for the native formatter measurement sidecar.

This validator proves the receipt's enrolled shape and typed counter contract.
It does not prove that counter values are authentic, mutually consistent, or
that fields beyond ``bench_id`` identify the benchmark subject.
"""

import argparse
import json
import re
import sys
from pathlib import Path


SCHEMA = "native-pipeline-measurements-v1"
ROW_SCHEMA = "native-pipeline-counters-v1"
_COUNTERS_SOURCE = (
    Path(__file__).resolve().parents[2]
    / "crates"
    / "perl-lsp-perltidy"
    / "src"
    / "native"
    / "counters.rs"
)
_CLOCK_TAG_DECLARATION = re.compile(
    r'^pub const COUNTER_CLOCK_TAG: &str = "(?P<tag>[^"]+)";', re.MULTILINE
)
COUNTER_FIELDS = (
    "pipeline_invocations",
    "parse_gate_invocations",
    "source_parse_gate_invocations",
    "formatted_output_parse_gate_invocations",
    "gate_nodes_observed",
    "lines_processed",
    "layout_groups_fitted",
    "edits_derived",
    "replacement_bytes",
    "peak_depth",
    "elapsed",
)


def counter_clock_tag() -> str:
    """Read the producer's named clock contract.

    The benchmark receipt is produced by Rust, so the Rust declaration is the
    authority. Keeping the validator coupled to that declaration makes a
    contract change fail closed until the producer and validator are reviewed
    together. This validates shape and labeling only; it does not authenticate
    counter values or prove subject identity beyond ``bench_id``.
    """
    try:
        source = _COUNTERS_SOURCE.read_text(encoding="utf-8")
    except OSError as error:
        raise ValueError(
            f"cannot read the Rust counter clock contract {_COUNTERS_SOURCE}: {error}"
        ) from error
    match = _CLOCK_TAG_DECLARATION.search(source)
    if match is None:
        raise ValueError(
            f"Rust counter clock contract is missing or malformed: {_COUNTERS_SOURCE}"
        )
    return match.group("tag")


def validate(path: Path, expected_run_id: str, expected_ids: list[str]) -> None:
    expected_clock_tag = counter_clock_tag()
    try:
        payload = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read valid JSON sidecar {path}: {error}") from error

    if not isinstance(payload, dict):
        raise ValueError("sidecar must be a JSON object")
    if payload.get("schema") != SCHEMA:
        raise ValueError(f"sidecar schema must be {SCHEMA!r}")
    if payload.get("run_id") != expected_run_id or not expected_run_id:
        raise ValueError("sidecar run_id does not match the current workflow run")

    rows = payload.get("subjects")
    if not isinstance(rows, list):
        raise ValueError("sidecar subjects must be an array")
    expected = set(expected_ids)
    if len(expected) != len(expected_ids):
        raise ValueError("expected subject IDs must be unique")
    if len(rows) != len(expected_ids):
        raise ValueError(
            f"sidecar must contain exactly {len(expected_ids)} enrolled rows; got {len(rows)}"
        )

    actual: list[str] = []
    for row in rows:
        if not isinstance(row, dict):
            raise ValueError("every sidecar subject row must be an object")
        if row.get("schema") != ROW_SCHEMA:
            raise ValueError("every sidecar row must carry the counter schema")
        if row.get("run_id") != expected_run_id:
            raise ValueError("every sidecar row must carry the current workflow run_id")
        counters = row.get("counters")
        if not isinstance(counters, dict):
            raise ValueError("every sidecar row must carry a counter snapshot")
        if counters.get("schema") != ROW_SCHEMA:
            raise ValueError(
                "every sidecar counter snapshot must carry the counter schema"
            )
        for field in COUNTER_FIELDS:
            if field not in counters:
                raise ValueError(f"every sidecar counter snapshot must carry {field}")
        if "clock_tag" not in counters:
            raise ValueError("every sidecar counter snapshot must carry clock_tag")
        if counters["clock_tag"] != expected_clock_tag:
            raise ValueError(
                f"sidecar counter clock_tag must be {expected_clock_tag!r}"
            )
        for field in COUNTER_FIELDS[:-1]:
            value = counters[field]
            if not isinstance(value, int) or isinstance(value, bool) or value < 0:
                raise ValueError(
                    f"sidecar counter {field} must be a non-negative integer"
                )
        if counters["pipeline_invocations"] == 0:
            raise ValueError("sidecar counter pipeline_invocations must be positive")
        elapsed = counters["elapsed"]
        if not isinstance(elapsed, dict):
            raise ValueError("sidecar counter elapsed must be a duration object")
        if set(elapsed) != {"secs", "nanos"} or any(
            not isinstance(elapsed[field], int)
            or isinstance(elapsed[field], bool)
            or elapsed[field] < 0
            for field in ("secs", "nanos")
        ):
            raise ValueError(
                "sidecar counter elapsed must contain non-negative secs and nanos"
            )
        # serde normalises `std::time::Duration`, so a real one always carries a
        # sub-second nanos remainder. A value at or above one billion did not
        # come from a Duration, which makes it fabricated or corrupted evidence
        # rather than a slow run — exactly what a typed guard exists to reject.
        if elapsed["nanos"] >= 1_000_000_000:
            raise ValueError(
                "sidecar counter elapsed nanos must be a sub-second remainder"
            )
        bench_id = row.get("bench_id")
        if not isinstance(bench_id, str) or not bench_id:
            raise ValueError("every sidecar row must carry a non-empty bench_id")
        toolchain = row.get("toolchain")
        if not isinstance(toolchain, str) or not toolchain:
            raise ValueError("every sidecar row must carry a non-empty toolchain")
        actual.append(bench_id)

    if len(set(actual)) != len(actual):
        raise ValueError("sidecar subject rows must be one-to-one; duplicate bench_id")
    if set(actual) != expected:
        missing = sorted(expected - set(actual))
        extra = sorted(set(actual) - expected)
        raise ValueError(
            f"sidecar enrollment mismatch; missing={missing}, extra={extra}"
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sidecar", type=Path, required=True)
    parser.add_argument("--expected-run-id", required=True)
    parser.add_argument("--expect-id", action="append", required=True)
    args = parser.parse_args()
    try:
        validate(args.sidecar, args.expected_run_id, args.expect_id)
    except ValueError as error:
        print(f"native pipeline sidecar invalid: {error}", file=sys.stderr)
        return 1
    print(f"validated native pipeline sidecar: {len(args.expect_id)} enrolled rows")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
