"""Shared policy and receipt helpers for the exact-binary DAP scorecard."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import subprocess
import time
from pathlib import Path
from typing import Any, Mapping, Sequence

SCHEMA_VERSION = "dap_runtime_scorecard.v2"
THRESHOLD_PCT = 80
LAUNCH_FIXTURES = ("hello", "loops", "eval", "args", "begin_end")
ATTACH_ATTEMPTS = 5
DEFAULT_TIMEOUT_SECONDS = 12.0
REQUIRED_PROCESS_INVOCATIONS = len(LAUNCH_FIXTURES) + ATTACH_ATTEMPTS + 1


class ScorecardError(RuntimeError):
    """A fail-closed runtime scorecard error."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
    except OSError as exc:
        raise ScorecardError(f"cannot hash {path}: {exc}") from exc
    return digest.hexdigest()


def percentile(values: Sequence[int], pct: int) -> int | None:
    if not values:
        return None
    ordered = sorted(values)
    rank = math.ceil((pct / 100.0) * len(ordered))
    return ordered[max(0, min(rank - 1, len(ordered) - 1))]


def run_text(argv: Sequence[str]) -> str:
    try:
        result = subprocess.run(
            list(argv),
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            timeout=15,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise ScorecardError(f"cannot execute {' '.join(argv)!r}: {exc}") from exc
    output = result.stdout.strip()
    if result.returncode != 0:
        raise ScorecardError(
            f"command {' '.join(argv)!r} failed with exit {result.returncode}: "
            f"{output or '<no output>'}"
        )
    return output


def metric_failure(detail: str) -> dict[str, str]:
    return {"status": "FAIL", "detail": detail}


def rate(details: list[dict[str, Any]]) -> dict[str, Any]:
    passed = sum(1 for row in details if row["error"] is None)
    latencies = [int(row["elapsed_ms"]) for row in details if row["error"] is None]
    return {
        "passed": passed,
        "total": len(details),
        "threshold_pct": THRESHOLD_PCT,
        "p50_ms": percentile(latencies, 50),
        "p95_ms": percentile(latencies, 95),
        "details": details,
    }


def validate_runtime_inputs(binary: Path, perl: Path, fixtures: Mapping[str, Path]) -> None:
    if set(fixtures) != set(LAUNCH_FIXTURES):
        raise ScorecardError(
            f"launch fixture names must be exactly {list(LAUNCH_FIXTURES)!r}, "
            f"got {sorted(fixtures)!r}"
        )
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ScorecardError(f"exact perl-dap binary is missing or not executable: {binary}")
    if not perl.is_file() or not os.access(perl, os.X_OK):
        raise ScorecardError(f"Perl runtime is missing or not executable: {perl}")


def scorecard_failures(scorecard: Mapping[str, Any]) -> list[str]:
    failures: list[str] = []
    for name in ("launch", "attach"):
        metric = scorecard.get(name)
        if not isinstance(metric, dict):
            failures.append(f"{name} metric missing")
            continue
        passed = metric.get("passed")
        total = metric.get("total")
        if not isinstance(passed, int) or not isinstance(total, int) or total <= 0:
            failures.append(f"{name} rate malformed")
            continue
        required = math.ceil(total * THRESHOLD_PCT / 100)
        if passed < required:
            failures.append(f"{name} below threshold: {passed}/{total}, need {required}")
    launch = scorecard.get("launch")
    if isinstance(launch, dict):
        p50 = launch.get("p50_ms")
        p95 = launch.get("p95_ms")
        if not isinstance(p50, int) or p50 > 2_000:
            failures.append(f"launch p50 exceeded 2000 ms or was absent: {p50!r}")
        if not isinstance(p95, int) or p95 > 5_000:
            failures.append(f"launch p95 exceeded 5000 ms or was absent: {p95!r}")
    for name, expected in (
        ("variables", "PASS"),
        ("evaluate", "PASS"),
        ("deep_pagination", "PASS"),
        ("memory", "MEASURED"),
    ):
        metric = scorecard.get(name)
        status = metric.get("status") if isinstance(metric, dict) else None
        if status != expected:
            detail = metric.get("detail") if isinstance(metric, dict) else "<missing>"
            failures.append(f"{name} expected {expected}, got {status!r}: {detail}")
    return failures


def write_json_atomic(path: Path, value: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = json.dumps(value, indent=2, sort_keys=True) + "\n"
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(payload, encoding="utf-8")
    temporary.replace(path)


def parse_fixture(raw: str) -> tuple[str, Path]:
    name, separator, path = raw.partition("=")
    if not separator or name not in LAUNCH_FIXTURES or not path:
        raise argparse.ArgumentTypeError(
            f"fixture must be NAME=PATH where NAME is one of {', '.join(LAUNCH_FIXTURES)}"
        )
    return name, Path(path)


def created_unix_seconds() -> int:
    return int(time.time())
