#!/usr/bin/env python3
"""Build and validate a candidate-bound DAP scorecard proof packet.

The Rust scorecard harness owns runtime measurements. This wrapper binds those
measurements to the exact repository revision, candidate ``perl-dap`` binary,
Perl runtime, fixture content, generated status page, and CI run that produced
them. It uses only the Python standard library so the proof lane does not add a
second dependency environment.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import platform
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence

SCHEMA_VERSION = "dap_scorecard_packet.v1"
REQUIRED_BINARY_STATUSES = {
    "variables": "PASS",
    "evaluate": "PASS",
    "deep_pagination": "PASS",
    "memory": "MEASURED",
}
REQUIRED_FIXTURES = (
    "crates/perl-dap/tests/fixtures/hello.pl",
    "crates/perl-dap/tests/fixtures/loops.pl",
    "crates/perl-dap/tests/fixtures/eval.pl",
    "crates/perl-dap/tests/fixtures/args.pl",
    "crates/perl-dap/tests/fixtures/breakpoints_begin_end.pl",
)
STATUS_MARKERS = (
    ("<!-- BEGIN: DAP_LAUNCH_SCORECARD -->", "<!-- END: DAP_LAUNCH_SCORECARD -->"),
    ("<!-- BEGIN: DAP_SESSION_SCORECARD -->", "<!-- END: DAP_SESSION_SCORECARD -->"),
)


class PacketError(RuntimeError):
    """A fail-closed scorecard packet validation error."""


def _read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise PacketError(f"missing JSON input: {path}") from exc
    except json.JSONDecodeError as exc:
        raise PacketError(f"malformed JSON in {path}: {exc}") from exc
    except OSError as exc:
        raise PacketError(f"cannot read {path}: {exc}") from exc


def _write_json(path: Path, value: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = json.dumps(value, indent=2, sort_keys=True) + "\n"
    path.write_text(payload, encoding="utf-8")


def _sha256(path: Path) -> str:
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


def _run_text(argv: Sequence[str]) -> str:
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
        raise PacketError(f"cannot execute {' '.join(argv)!r}: {exc}") from exc
    output = result.stdout.strip()
    if result.returncode != 0:
        raise PacketError(
            f"command {' '.join(argv)!r} failed with exit {result.returncode}: {output or '<no output>'}"
        )
    return output


def _as_object(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise PacketError(f"{context} must be a JSON object")
    return value


def _as_int(value: Any, context: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise PacketError(f"{context} must be an integer")
    return value


def _validate_rate(scorecard: Mapping[str, Any], name: str) -> None:
    metric = _as_object(scorecard.get(name), f"scorecard.{name}")
    passed = _as_int(metric.get("passed"), f"scorecard.{name}.passed")
    total = _as_int(metric.get("total"), f"scorecard.{name}.total")
    threshold = _as_int(metric.get("threshold_pct"), f"scorecard.{name}.threshold_pct")
    if total <= 0:
        raise PacketError(f"scorecard.{name} has no executed samples")
    if passed < 0 or passed > total:
        raise PacketError(f"scorecard.{name} has impossible passed/total values: {passed}/{total}")
    if not 0 <= threshold <= 100:
        raise PacketError(f"scorecard.{name}.threshold_pct is outside 0..100: {threshold}")
    required = math.ceil(total * threshold / 100)
    if passed < required:
        raise PacketError(
            f"scorecard.{name} is below threshold: {passed}/{total} passed, need at least {required}"
        )


def validate_scorecard(raw: Any) -> Mapping[str, Any]:
    scorecard = _as_object(raw, "scorecard receipt")
    if scorecard.get("perl_available") is not True:
        raise PacketError("scorecard receipt does not prove a usable Perl runtime")
    _validate_rate(scorecard, "launch")
    _validate_rate(scorecard, "attach")

    for name, expected in REQUIRED_BINARY_STATUSES.items():
        metric = _as_object(scorecard.get(name), f"scorecard.{name}")
        actual = metric.get("status")
        if actual != expected:
            detail = metric.get("detail", "<no detail>")
            raise PacketError(
                f"scorecard.{name} must be {expected}, got {actual!r}: {detail}"
            )
    return scorecard


def _relative_path(root: Path, path: Path) -> str:
    try:
        return path.resolve().relative_to(root.resolve()).as_posix()
    except ValueError as exc:
        raise PacketError(f"evidence path escapes repository root: {path}") from exc


def _status_blocks(status_text: str) -> list[str]:
    blocks: list[str] = []
    for begin, end in STATUS_MARKERS:
        start = status_text.find(begin)
        stop = status_text.find(end)
        if start < 0 or stop < 0 or stop <= start:
            raise PacketError(f"generated status is missing marker pair {begin!r} / {end!r}")
        blocks.append(status_text[start : stop + len(end)])
    return blocks


def validate_generated_status(status_path: Path) -> None:
    try:
        text = status_path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise PacketError(f"missing generated DAP status: {status_path}") from exc
    except OSError as exc:
        raise PacketError(f"cannot read generated DAP status {status_path}: {exc}") from exc

    for block in _status_blocks(text):
        if "receipt missing" in block:
            raise PacketError("generated DAP status still reports a missing receipt")
        if "| SKIP |" in block:
            raise PacketError("generated DAP status contains a SKIP verdict in a required scorecard block")
        if "| FAIL |" in block:
            raise PacketError("generated DAP status contains a FAIL verdict in a required scorecard block")


def _fixture_records(root: Path, fixture_paths: Iterable[str]) -> list[dict[str, str]]:
    records: list[dict[str, str]] = []
    seen: set[str] = set()
    for raw_path in fixture_paths:
        path = root / raw_path
        relative = _relative_path(root, path)
        if relative in seen:
            raise PacketError(f"duplicate fixture identity: {relative}")
        seen.add(relative)
        records.append({"path": relative, "sha256": _sha256(path)})

    expected = set(REQUIRED_FIXTURES)
    actual = {record["path"] for record in records}
    if actual != expected:
        missing = sorted(expected - actual)
        extra = sorted(actual - expected)
        raise PacketError(f"fixture set mismatch; missing={missing}, extra={extra}")
    records.sort(key=lambda record: record["path"])
    return records


def build_packet(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.repository_root).resolve()
    raw_path = (root / args.raw_receipt).resolve()
    status_path = (root / args.status).resolve()
    binary_path = (root / args.binary).resolve()
    perl_path = Path(args.perl).resolve()

    scorecard = validate_scorecard(_read_json(raw_path))
    validate_generated_status(status_path)

    if args.repository_dirty:
        raise PacketError("candidate repository was dirty before the scorecard run")
    if not args.repository_sha or len(args.repository_sha) < 7:
        raise PacketError("repository SHA is missing or implausibly short")
    if not args.run_id:
        raise PacketError("CI run identity is required")

    binary_version = _run_text([str(binary_path), "--version"])
    perl_version = _run_text([str(perl_path), "-e", "print $^V"])
    created = int(time.time())

    packet: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "created_unix_seconds": created,
        "repository": {
            "sha": args.repository_sha,
            "dirty": False,
        },
        "run": {
            "id": str(args.run_id),
            "attempt": str(args.run_attempt),
            "operating_system": platform.system(),
            "architecture": platform.machine(),
        },
        "binary": {
            "path": _relative_path(root, binary_path),
            "sha256": _sha256(binary_path),
            "version_output": binary_version,
        },
        "perl": {
            "path": str(perl_path),
            "version": perl_version,
        },
        "fixtures": _fixture_records(root, args.fixture),
        "raw_receipt": {
            "path": _relative_path(root, raw_path),
            "sha256": _sha256(raw_path),
        },
        "generated_status": {
            "path": _relative_path(root, status_path),
            "sha256": _sha256(status_path),
        },
        "scorecard": scorecard,
    }
    return packet


def _expect_equal(actual: Any, expected: Any, context: str) -> None:
    if actual != expected:
        raise PacketError(f"{context} mismatch: expected {expected!r}, got {actual!r}")


def _validate_subject_hash(root: Path, subject: Mapping[str, Any], context: str) -> None:
    path_value = subject.get("path")
    digest_value = subject.get("sha256")
    if not isinstance(path_value, str) or not isinstance(digest_value, str):
        raise PacketError(f"{context} path/hash fields are missing")
    _expect_equal(_sha256(root / path_value), digest_value, f"{context} SHA-256")


def validate_packet(args: argparse.Namespace) -> Mapping[str, Any]:
    root = Path(args.repository_root).resolve()
    packet = _as_object(_read_json(Path(args.packet)), "scorecard packet")
    _expect_equal(packet.get("schema_version"), SCHEMA_VERSION, "packet schema")

    repository = _as_object(packet.get("repository"), "packet.repository")
    _expect_equal(repository.get("sha"), args.expected_repository_sha, "repository SHA")
    _expect_equal(repository.get("dirty"), False, "repository dirty flag")

    run = _as_object(packet.get("run"), "packet.run")
    _expect_equal(str(run.get("id")), str(args.expected_run_id), "CI run ID")
    _expect_equal(str(run.get("attempt")), str(args.expected_run_attempt), "CI run attempt")

    created = _as_int(packet.get("created_unix_seconds"), "packet.created_unix_seconds")
    age = int(time.time()) - created
    if age < 0 or age > args.max_age_seconds:
        raise PacketError(
            f"scorecard packet is stale or future-dated: age={age}s, max={args.max_age_seconds}s"
        )

    binary = _as_object(packet.get("binary"), "packet.binary")
    _validate_subject_hash(root, binary, "candidate binary")
    _expect_equal(binary.get("sha256"), args.expected_binary_sha256, "candidate binary SHA-256")

    raw_receipt = _as_object(packet.get("raw_receipt"), "packet.raw_receipt")
    _validate_subject_hash(root, raw_receipt, "raw scorecard receipt")

    generated_status = _as_object(packet.get("generated_status"), "packet.generated_status")
    _validate_subject_hash(root, generated_status, "generated DAP status")
    status_path = root / str(generated_status.get("path"))
    validate_generated_status(status_path)

    fixtures = packet.get("fixtures")
    if not isinstance(fixtures, list):
        raise PacketError("packet.fixtures must be an array")
    seen: set[str] = set()
    fixture_paths: set[str] = set()
    for index, item in enumerate(fixtures):
        record = _as_object(item, f"packet.fixtures[{index}]")
        path_value = record.get("path")
        if not isinstance(path_value, str):
            raise PacketError(f"packet.fixtures[{index}].path is missing")
        if path_value in seen:
            raise PacketError(f"duplicate fixture identity: {path_value}")
        seen.add(path_value)
        fixture_paths.add(path_value)
        _validate_subject_hash(root, record, f"fixture {path_value}")
    _expect_equal(fixture_paths, set(REQUIRED_FIXTURES), "fixture identity set")

    validate_scorecard(packet.get("scorecard"))
    return packet


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    build = subparsers.add_parser("build", help="build a candidate-bound scorecard packet")
    build.add_argument("--repository-root", default=".")
    build.add_argument("--repository-sha", required=True)
    build.add_argument("--repository-dirty", action="store_true")
    build.add_argument("--run-id", required=True)
    build.add_argument("--run-attempt", default="1")
    build.add_argument("--binary", required=True)
    build.add_argument("--perl", required=True)
    build.add_argument("--raw-receipt", required=True)
    build.add_argument("--status", required=True)
    build.add_argument("--fixture", action="append", default=[])
    build.add_argument("--output", required=True)

    validate = subparsers.add_parser("validate", help="validate packet identity and evidence")
    validate.add_argument("--repository-root", default=".")
    validate.add_argument("--packet", required=True)
    validate.add_argument("--expected-repository-sha", required=True)
    validate.add_argument("--expected-binary-sha256", required=True)
    validate.add_argument("--expected-run-id", required=True)
    validate.add_argument("--expected-run-attempt", default="1")
    validate.add_argument("--max-age-seconds", type=int, default=7200)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        if args.command == "build":
            packet = build_packet(args)
            _write_json(Path(args.output), packet)
            print(f"DAP scorecard packet: {args.output}")
        else:
            validate_packet(args)
            print(f"DAP scorecard packet valid: {args.packet}")
    except PacketError as exc:
        print(f"DAP scorecard packet error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
