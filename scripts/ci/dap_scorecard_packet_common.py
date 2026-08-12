"""Shared constants, process helpers, and status validation for DAP packets."""

from __future__ import annotations

import hashlib
import json
import math
import subprocess
from pathlib import Path
from typing import Any, Mapping, Sequence

from dap_scorecard_model import (
    ATTACH_ATTEMPTS,
    LAUNCH_FIXTURES,
    REQUIRED_PROCESS_INVOCATIONS,
    SCHEMA_VERSION as RUNTIME_SCHEMA_VERSION,
    THRESHOLD_PCT as REQUIRED_THRESHOLD_PCT,
)

SCHEMA_VERSION = "dap_scorecard_packet.v2"
REQUIRED_BINARY_STATUSES = {
    "variables": "PASS",
    "evaluate": "PASS",
    "deep_pagination": "PASS",
    "memory": "MEASURED",
}
REQUIRED_LAUNCH_FIXTURE_NAMES = LAUNCH_FIXTURES
REQUIRED_ATTACH_NAMES = ("tcp_loopback",) * ATTACH_ATTEMPTS
REQUIRED_FIXTURES = (
    "crates/perl-dap/tests/fixtures/hello.pl",
    "crates/perl-dap/tests/fixtures/loops.pl",
    "crates/perl-dap/tests/fixtures/eval.pl",
    "crates/perl-dap/tests/fixtures/args.pl",
    "crates/perl-dap/tests/fixtures/breakpoints_begin_end.pl",
)
REQUIRED_SOURCE_SUBJECTS = (
    ".github/workflows/dap-scorecard.yml",
    "scripts/ci/dap_scorecard_model.py",
    "scripts/ci/dap_scorecard_packet.py",
    "scripts/ci/dap_scorecard_packet_common.py",
    "scripts/ci/dap_scorecard_packet_git.py",
    "scripts/ci/dap_scorecard_packet_policy.py",
    "scripts/ci/dap_scorecard_probes.py",
    "scripts/ci/dap_scorecard_runtime.py",
    "scripts/ci/dap_scorecard_transport.py",
    "scripts/tests/test_dap_scorecard_packet.py",
    "scripts/tests/test_dap_scorecard_runtime.py",
    "xtask/src/tasks/update_status/dap.rs",
)
GENERATED_STATUS_PATH = "docs/project/status/dap.md"
STATUS_MARKERS = (
    ("<!-- BEGIN: DAP_LAUNCH_SCORECARD -->", "<!-- END: DAP_LAUNCH_SCORECARD -->"),
    ("<!-- BEGIN: DAP_SESSION_SCORECARD -->", "<!-- END: DAP_SESSION_SCORECARD -->"),
)


class PacketError(RuntimeError):
    """A fail-closed scorecard packet validation error."""


def read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise PacketError(f"missing JSON input: {path}") from exc
    except json.JSONDecodeError as exc:
        raise PacketError(f"malformed JSON in {path}: {exc}") from exc
    except OSError as exc:
        raise PacketError(f"cannot read {path}: {exc}") from exc


def write_json(path: Path, value: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
    except FileNotFoundError as exc:
        raise PacketError(f"missing evidence subject: {path}") from exc
    except OSError as exc:
        raise PacketError(f"cannot hash {path}: {exc}") from exc
    return digest.hexdigest()


def run(
    argv: Sequence[str],
    *,
    cwd: Path | None = None,
    timeout: int = 20,
    text: bool = True,
) -> subprocess.CompletedProcess[Any]:
    try:
        return subprocess.run(
            list(argv),
            cwd=cwd,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=text,
            timeout=timeout,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise PacketError(f"cannot execute {' '.join(argv)!r}: {exc}") from exc


def run_text(argv: Sequence[str], *, cwd: Path | None = None) -> str:
    result = run(argv, cwd=cwd, text=True)
    output = result.stdout.strip()
    if result.returncode != 0:
        raise PacketError(
            f"command {' '.join(argv)!r} failed with exit {result.returncode}: "
            f"{output or '<no output>'}"
        )
    return output


def run_bytes(argv: Sequence[str], *, cwd: Path | None = None) -> bytes:
    result = run(argv, cwd=cwd, text=False)
    output = bytes(result.stdout)
    if result.returncode != 0:
        rendered = output.decode("utf-8", errors="replace").strip()
        raise PacketError(
            f"command {' '.join(argv)!r} failed with exit {result.returncode}: "
            f"{rendered or '<no output>'}"
        )
    return output


def as_object(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise PacketError(f"{context} must be a JSON object")
    return value


def as_int(value: Any, context: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise PacketError(f"{context} must be an integer")
    return value


def as_nonnegative_int(value: Any, context: str) -> int:
    result = as_int(value, context)
    if result < 0:
        raise PacketError(f"{context} must be nonnegative")
    return result


def percentile(values: Sequence[int], pct: int) -> int | None:
    if not values:
        return None
    ordered = sorted(values)
    rank = math.ceil((pct / 100.0) * len(ordered))
    return ordered[max(0, min(rank - 1, len(ordered) - 1))]


def relative_path(root: Path, path: Path) -> str:
    try:
        return path.resolve().relative_to(root.resolve()).as_posix()
    except ValueError as exc:
        raise PacketError(f"evidence path escapes repository root: {path}") from exc


def expect_equal(actual: Any, expected: Any, context: str) -> None:
    if actual != expected:
        raise PacketError(f"{context} mismatch: expected {expected!r}, got {actual!r}")


def validate_subject_hash(root: Path, subject: Mapping[str, Any], context: str) -> None:
    path_value = subject.get("path")
    digest_value = subject.get("sha256")
    if not isinstance(path_value, str) or not isinstance(digest_value, str):
        raise PacketError(f"{context} path/hash fields are missing")
    expect_equal(sha256(root / path_value), digest_value, f"{context} SHA-256")


def _format_rate(metric: Mapping[str, Any], context: str) -> str:
    passed = as_nonnegative_int(metric.get("passed"), f"{context}.passed")
    total = as_nonnegative_int(metric.get("total"), f"{context}.total")
    if total == 0:
        return "SKIP"
    pct = (passed * 100) // total
    return f"{passed}/{total} ({pct} %)"


def _status_for_rate(metric: Mapping[str, Any], context: str) -> str:
    passed = as_nonnegative_int(metric.get("passed"), f"{context}.passed")
    total = as_nonnegative_int(metric.get("total"), f"{context}.total")
    threshold = as_nonnegative_int(metric.get("threshold_pct"), f"{context}.threshold_pct")
    if total == 0:
        return "SKIP"
    return "PASS" if (passed * 100) // total >= threshold else "FAIL"


def _format_latency(value: Any, context: str, limit_ms: int) -> tuple[str, str]:
    if value is None:
        return "—", "SKIP"
    latency = as_nonnegative_int(value, context)
    return f"{latency} ms", "PASS" if latency <= limit_ms else "FAIL"


def _metric_detail(scorecard: Mapping[str, Any], name: str) -> tuple[Mapping[str, Any], str]:
    metric = as_object(scorecard.get(name), f"scorecard.{name}")
    detail = metric.get("detail")
    if not isinstance(detail, str) or not detail:
        raise PacketError(f"scorecard.{name}.detail is missing")
    if "|" in detail or "\r" in detail or "\n" in detail:
        raise PacketError(f"scorecard.{name}.detail is not a safe single-line Markdown cell")
    return metric, detail


def expected_generated_status_blocks(scorecard: Mapping[str, Any]) -> Mapping[str, str]:
    """Render the two generated scorecard blocks exactly as xtask does."""

    launch = as_object(scorecard.get("launch"), "scorecard.launch")
    attach = as_object(scorecard.get("attach"), "scorecard.attach")
    variables, variables_detail = _metric_detail(scorecard, "variables")
    evaluate, evaluate_detail = _metric_detail(scorecard, "evaluate")
    deep_pagination, deep_detail = _metric_detail(scorecard, "deep_pagination")
    memory, memory_detail = _metric_detail(scorecard, "memory")

    launch_threshold = as_nonnegative_int(
        launch.get("threshold_pct"), "scorecard.launch.threshold_pct"
    )
    attach_threshold = as_nonnegative_int(
        attach.get("threshold_pct"), "scorecard.attach.threshold_pct"
    )
    p50, p50_status = _format_latency(
        launch.get("p50_ms"), "scorecard.launch.p50_ms", 2_000
    )
    p95, p95_status = _format_latency(
        launch.get("p95_ms"), "scorecard.launch.p95_ms", 5_000
    )
    availability_note = "" if scorecard.get("perl_available") is True else " (perl unavailable)"

    launch_block = "\n".join(
        (
            "| Metric | Value | Target | Status |",
            "|---|---|---|---|",
            (
                f"| Launch success rate | {_format_rate(launch, 'scorecard.launch')} "
                f"| ≥ {launch_threshold} % | {_status_for_rate(launch, 'scorecard.launch')} |"
            ),
            "| Fixtures tested | hello, loops, eval, args, begin_end | 5 | — |",
            f"| cold_launch_p50 | {p50} | ≤ 2 000 ms | {p50_status} |",
            f"| cold_launch_p95 | {p95} | ≤ 5 000 ms | {p95_status} |",
        )
    )
    session_block = "\n".join(
        (
            "| Metric | Value | Target | Status |",
            "|---|---|---|---|",
            (
                "| Attach success rate (TCP loopback) | "
                f"{_format_rate(attach, 'scorecard.attach')}{availability_note} "
                f"| ≥ {attach_threshold} % | {_status_for_rate(attach, 'scorecard.attach')} |"
            ),
            (
                "| Variables pane correctness (real session) | "
                f"{variables_detail} | expected named variables in scope "
                f"| {variables.get('status')} |"
            ),
            (
                "| Evaluate correctness (real session) | "
                f"{evaluate_detail} | evaluate($x + 1) => 42 "
                f"| {evaluate.get('status')} |"
            ),
            (
                "| Deep truncation/pagination correctness | "
                f"{deep_detail} | page [250..274] over @big "
                f"| {deep_pagination.get('status')} |"
            ),
            (
                "| Memory footprint baseline (portable proxy) | "
                f"{memory_detail} | best-effort baseline | {memory.get('status')} |"
            ),
        )
    )
    return {
        STATUS_MARKERS[0][0]: launch_block,
        STATUS_MARKERS[1][0]: session_block,
    }


def validate_generated_status(
    status_path: Path, scorecard: Mapping[str, Any] | None = None
) -> None:
    try:
        text = status_path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise PacketError(f"missing generated DAP status: {status_path}") from exc
    except OSError as exc:
        raise PacketError(f"cannot read generated DAP status {status_path}: {exc}") from exc

    expected_blocks = expected_generated_status_blocks(scorecard) if scorecard is not None else {}
    for begin, end in STATUS_MARKERS:
        if text.count(begin) != 1 or text.count(end) != 1:
            raise PacketError(
                f"generated status must contain exactly one marker pair {begin!r} / {end!r}"
            )
        start = text.find(begin)
        stop = text.find(end, start + len(begin))
        if stop <= start:
            raise PacketError(f"generated status is missing marker pair {begin!r} / {end!r}")
        raw_body = text[start + len(begin) : stop]
        if not raw_body.startswith("\n") or not raw_body.endswith("\n"):
            raise PacketError(f"generated status block {begin!r} has noncanonical boundaries")
        block = raw_body[1:-1]
        if "receipt missing" in block:
            raise PacketError("generated DAP status still reports a missing receipt")
        if "| SKIP |" in block:
            raise PacketError("generated DAP status contains SKIP in a required scorecard block")
        if "| FAIL |" in block:
            raise PacketError("generated DAP status contains FAIL in a required scorecard block")
        expected = expected_blocks.get(begin)
        if expected is not None:
            expect_equal(block, expected, f"generated status block {begin}")