#!/usr/bin/env python3
"""Drive the exact perl-dap executable over stdio and emit a runtime scorecard."""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path
from typing import Any, Mapping, Sequence

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from dap_scorecard_model import (  # noqa: E402
    ATTACH_ATTEMPTS,
    DEFAULT_TIMEOUT_SECONDS,
    LAUNCH_FIXTURES,
    MAX_SCORECARD_DURATION_MS,
    REQUIRED_PROCESS_INVOCATIONS,
    SCHEMA_VERSION,
    ScorecardError,
    metric_failure,
    parse_fixture,
    percentile,
    rate,
    run_text,
    scorecard_failures,
    sha256_file,
    validate_runtime_inputs,
    write_json_atomic,
)
from dap_scorecard_probes import probe_attach, probe_launch, probe_session_metrics  # noqa: E402
from dap_scorecard_transport import (  # noqa: E402
    InvocationCounter,
    frame_message,
    read_framed_message,
)

# Compatibility aliases for focused tests and downstream imports.
_parse_fixture = parse_fixture


def build_scorecard(
    binary: Path,
    perl: Path,
    fixtures: Mapping[str, Path],
    timeout_seconds: float,
) -> dict[str, Any]:
    started_unix_ms = time.time_ns() // 1_000_000
    started_monotonic_ns = time.monotonic_ns()

    binary = binary.resolve()
    perl = perl.resolve()
    validate_runtime_inputs(binary, perl, fixtures)
    binary_sha256 = sha256_file(binary)
    version_output = run_text([str(binary), "--version"])
    perl_version = run_text([str(perl), "-e", "print $^V"])
    invocations = InvocationCounter()

    launch_details: list[dict[str, Any]] = []
    for name in LAUNCH_FIXTURES:
        try:
            elapsed = probe_launch(
                binary,
                fixtures[name].resolve(),
                timeout_seconds,
                invocations,
            )
            launch_details.append({"name": name, "elapsed_ms": elapsed, "error": None})
        except ScorecardError as exc:
            launch_details.append({"name": name, "elapsed_ms": None, "error": str(exc)})

    attach_details: list[dict[str, Any]] = []
    for _attempt in range(ATTACH_ATTEMPTS):
        try:
            elapsed = probe_attach(binary, timeout_seconds, invocations)
            attach_details.append(
                {"name": "tcp_loopback", "elapsed_ms": elapsed, "error": None}
            )
        except ScorecardError as exc:
            attach_details.append(
                {"name": "tcp_loopback", "elapsed_ms": None, "error": str(exc)}
            )

    try:
        variables, evaluate, deep_pagination, memory = probe_session_metrics(
            binary, timeout_seconds, invocations
        )
    except ScorecardError as exc:
        detail = f"exact-binary stdio session setup/teardown failed: {exc}"
        variables = metric_failure(detail)
        evaluate = metric_failure(detail)
        deep_pagination = metric_failure(detail)
        memory = metric_failure(detail)

    ended_monotonic_ns = time.monotonic_ns()
    ended_unix_ms = time.time_ns() // 1_000_000
    duration_ms = max(0, (ended_monotonic_ns - started_monotonic_ns) // 1_000_000)

    return {
        "schema_version": SCHEMA_VERSION,
        "created_unix_seconds": ended_unix_ms // 1000,
        "timing": {
            "started_unix_ms": started_unix_ms,
            "ended_unix_ms": ended_unix_ms,
            "duration_ms": duration_ms,
            "max_duration_ms": MAX_SCORECARD_DURATION_MS,
        },
        "subject": {
            "transport": "stdio",
            "binary_path": str(binary),
            "binary_sha256": binary_sha256,
            "version_output": version_output,
            "process_invocations": invocations.count,
        },
        "perl_available": True,
        "perl": {"path": str(perl), "version": perl_version},
        "launch": rate(launch_details),
        "attach": rate(attach_details),
        "variables": variables,
        "evaluate": evaluate,
        "deep_pagination": deep_pagination,
        "memory": memory,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--perl", required=True)
    parser.add_argument("--fixture", action="append", type=parse_fixture, default=[])
    parser.add_argument("--receipt", required=True)
    parser.add_argument("--timeout-seconds", type=float, default=DEFAULT_TIMEOUT_SECONDS)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        fixtures: dict[str, Path] = {}
        for name, path in args.fixture:
            if name in fixtures:
                raise ScorecardError(f"duplicate launch fixture name: {name}")
            fixtures[name] = path
        scorecard = build_scorecard(
            Path(args.binary), Path(args.perl), fixtures, args.timeout_seconds
        )
        write_json_atomic(Path(args.receipt), scorecard)
        failures = scorecard_failures(scorecard)
        print(f"DAP exact-binary scorecard receipt: {args.receipt}")
        print(
            f"DAP launch: {scorecard['launch']['passed']}/{scorecard['launch']['total']}; "
            f"attach: {scorecard['attach']['passed']}/{scorecard['attach']['total']}; "
            f"exact stdio invocations: {scorecard['subject']['process_invocations']}; "
            f"duration: {scorecard['timing']['duration_ms']} ms"
        )
        if failures:
            for failure in failures:
                print(f"DAP scorecard failure: {failure}", file=sys.stderr)
            return 1
    except ScorecardError as exc:
        print(f"DAP scorecard error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
