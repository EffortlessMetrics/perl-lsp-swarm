"""Scorecard denominator, aggregate, subject, and exact-binary policy."""

from __future__ import annotations

import math
from pathlib import Path
from typing import Any, Mapping, Sequence

from dap_scorecard_packet_common import (
    REQUIRED_ATTACH_NAMES,
    REQUIRED_BINARY_STATUSES,
    REQUIRED_LAUNCH_FIXTURE_NAMES,
    REQUIRED_PROCESS_INVOCATIONS,
    REQUIRED_THRESHOLD_PCT,
    RUNTIME_SCHEMA_VERSION,
    PacketError,
    as_int,
    as_nonnegative_int,
    as_object,
    percentile,
    run_text,
)


def _validate_rate(
    scorecard: Mapping[str, Any], name: str, *, expected_names: Sequence[str]
) -> None:
    metric = as_object(scorecard.get(name), f"scorecard.{name}")
    passed = as_int(metric.get("passed"), f"scorecard.{name}.passed")
    total = as_int(metric.get("total"), f"scorecard.{name}.total")
    threshold = as_int(metric.get("threshold_pct"), f"scorecard.{name}.threshold_pct")
    if threshold != REQUIRED_THRESHOLD_PCT:
        raise PacketError(
            f"scorecard.{name}.threshold_pct must be {REQUIRED_THRESHOLD_PCT}, got {threshold}"
        )
    if total != len(expected_names):
        raise PacketError(f"scorecard.{name}.total must be {len(expected_names)}, got {total}")
    if passed < 0 or passed > total:
        raise PacketError(f"scorecard.{name} has impossible passed/total values: {passed}/{total}")
    details = metric.get("details")
    if not isinstance(details, list) or len(details) != total:
        raise PacketError(f"scorecard.{name}.details must contain exactly {total} rows")

    names: list[str] = []
    observed_passed = 0
    latencies: list[int] = []
    for index, raw_detail in enumerate(details):
        detail = as_object(raw_detail, f"scorecard.{name}.details[{index}]")
        detail_name = detail.get("name")
        if not isinstance(detail_name, str) or not detail_name:
            raise PacketError(f"scorecard.{name}.details[{index}].name is missing")
        names.append(detail_name)
        error = detail.get("error")
        elapsed = detail.get("elapsed_ms")
        if error is None:
            observed_passed += 1
            latencies.append(
                as_nonnegative_int(elapsed, f"scorecard.{name}.details[{index}].elapsed_ms")
            )
        else:
            if not isinstance(error, str) or not error:
                raise PacketError(f"scorecard.{name}.details[{index}].error is invalid")
            if elapsed is not None:
                as_nonnegative_int(elapsed, f"scorecard.{name}.details[{index}].elapsed_ms")

    if names != list(expected_names):
        raise PacketError(f"scorecard.{name} rows differ from the required ordered set: {names!r}")
    if observed_passed != passed:
        raise PacketError(
            f"scorecard.{name} pass count contradicts its rows: {passed} != {observed_passed}"
        )
    expected_p50 = percentile(latencies, 50)
    expected_p95 = percentile(latencies, 95)
    if metric.get("p50_ms") != expected_p50:
        raise PacketError(
            f"scorecard.{name}.p50_ms contradicts detail rows: "
            f"expected {expected_p50!r}, got {metric.get('p50_ms')!r}"
        )
    if metric.get("p95_ms") != expected_p95:
        raise PacketError(
            f"scorecard.{name}.p95_ms contradicts detail rows: "
            f"expected {expected_p95!r}, got {metric.get('p95_ms')!r}"
        )
    required = math.ceil(total * REQUIRED_THRESHOLD_PCT / 100)
    if passed < required:
        raise PacketError(
            f"scorecard.{name} is below threshold: {passed}/{total} passed, need {required}"
        )
    if name == "launch":
        if expected_p50 is None or expected_p50 > 2_000:
            raise PacketError(f"scorecard.launch p50 exceeds 2000 ms: {expected_p50!r}")
        if expected_p95 is None or expected_p95 > 5_000:
            raise PacketError(f"scorecard.launch p95 exceeds 5000 ms: {expected_p95!r}")


def validate_scorecard(raw: Any) -> Mapping[str, Any]:
    scorecard = as_object(raw, "scorecard receipt")
    if scorecard.get("schema_version") != RUNTIME_SCHEMA_VERSION:
        raise PacketError(
            f"scorecard receipt schema must be {RUNTIME_SCHEMA_VERSION!r}, "
            f"got {scorecard.get('schema_version')!r}"
        )
    subject = as_object(scorecard.get("subject"), "scorecard.subject")
    if subject.get("transport") != "stdio":
        raise PacketError("scorecard subject must use the real stdio transport")
    if not isinstance(subject.get("binary_path"), str) or not subject.get("binary_path"):
        raise PacketError("scorecard.subject.binary_path is missing")
    if not isinstance(subject.get("binary_sha256"), str) or len(subject["binary_sha256"]) != 64:
        raise PacketError("scorecard.subject.binary_sha256 is missing or malformed")
    if not isinstance(subject.get("version_output"), str) or not subject.get("version_output"):
        raise PacketError("scorecard.subject.version_output is missing")
    invocations = as_int(subject.get("process_invocations"), "scorecard.subject.process_invocations")
    if invocations != REQUIRED_PROCESS_INVOCATIONS:
        raise PacketError(
            f"scorecard must record {REQUIRED_PROCESS_INVOCATIONS} exact-binary invocations, "
            f"got {invocations}"
        )
    if scorecard.get("perl_available") is not True:
        raise PacketError("scorecard receipt does not prove a usable Perl runtime")
    perl = as_object(scorecard.get("perl"), "scorecard.perl")
    if not isinstance(perl.get("path"), str) or not isinstance(perl.get("version"), str):
        raise PacketError("scorecard.perl path/version are missing")

    _validate_rate(scorecard, "launch", expected_names=REQUIRED_LAUNCH_FIXTURE_NAMES)
    _validate_rate(scorecard, "attach", expected_names=REQUIRED_ATTACH_NAMES)
    for name, expected in REQUIRED_BINARY_STATUSES.items():
        metric = as_object(scorecard.get(name), f"scorecard.{name}")
        if metric.get("status") != expected:
            raise PacketError(
                f"scorecard.{name} must be {expected}, got {metric.get('status')!r}: "
                f"{metric.get('detail', '<no detail>')}"
            )
    return scorecard


def validate_exact_binary_subject(
    scorecard: Mapping[str, Any], binary_path: Path, binary_sha256: str
) -> None:
    subject = as_object(scorecard.get("subject"), "scorecard.subject")
    recorded_path = Path(str(subject.get("binary_path"))).resolve()
    if recorded_path != binary_path.resolve():
        raise PacketError(
            f"runtime receipt measured {recorded_path}, packet binds {binary_path.resolve()}"
        )
    if subject.get("binary_sha256") != binary_sha256:
        raise PacketError("runtime receipt binary SHA-256 differs from the packet candidate binary")
    if subject.get("version_output") != run_text([str(binary_path), "--version"]):
        raise PacketError("runtime receipt binary version output differs from the exact binary")
