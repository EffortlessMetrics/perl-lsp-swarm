#!/usr/bin/env python3
"""Build and validate a candidate-bound exact-binary DAP scorecard packet."""

from __future__ import annotations

import argparse
import platform
import sys
import time
from pathlib import Path
from typing import Any, Mapping, Sequence

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from dap_scorecard_packet_common import (  # noqa: E402
    GENERATED_STATUS_PATH,
    REQUIRED_ATTACH_NAMES,
    REQUIRED_FIXTURES,
    REQUIRED_LAUNCH_FIXTURE_NAMES,
    REQUIRED_PROCESS_INVOCATIONS,
    REQUIRED_SOURCE_SUBJECTS,
    REQUIRED_THRESHOLD_PCT,
    RUNTIME_SCHEMA_VERSION,
    SCHEMA_VERSION,
    PacketError,
    as_int,
    as_object,
    expect_equal,
    read_json,
    relative_path,
    run_text,
    sha256,
    validate_generated_status,
    validate_subject_hash,
    write_json,
)
from dap_scorecard_packet_git import (  # noqa: E402
    assert_repository_state,
    tracked_record,
    tracked_records,
    validate_tracked_packet_records,
    verify_head,
)
from dap_scorecard_packet_policy import (  # noqa: E402
    validate_exact_binary_subject,
    validate_scorecard,
)

# Compatibility aliases for focused tests and downstream imports.
_read_json = read_json
_write_json = write_json
_sha256 = sha256


def build_packet(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.repository_root).resolve()
    raw_path = (root / args.raw_receipt).resolve()
    status_path = (root / args.status).resolve()
    binary_path = (root / args.binary).resolve()
    perl_path = Path(args.perl).resolve()
    if args.repository_dirty:
        raise PacketError("caller reported a dirty candidate before the scorecard run")
    if len(args.repository_sha) != 40:
        raise PacketError("repository SHA must be a full 40-character commit identity")
    if not args.run_id:
        raise PacketError("CI run identity is required")

    verify_head(root, args.repository_sha)
    status_lines = assert_repository_state(root)
    scorecard = validate_scorecard(read_json(raw_path))
    validate_generated_status(status_path)
    binary_sha256 = sha256(binary_path)
    validate_exact_binary_subject(scorecard, binary_path, binary_sha256)
    binary_version = run_text([str(binary_path), "--version"])
    perl_version = run_text([str(perl_path), "-e", "print $^V"])
    scorecard_perl = as_object(scorecard.get("perl"), "scorecard.perl")
    if Path(str(scorecard_perl.get("path"))).resolve() != perl_path:
        raise PacketError("runtime receipt Perl path differs from packet Perl path")
    if scorecard_perl.get("version") != perl_version:
        raise PacketError("runtime receipt Perl version differs from packet Perl runtime")

    return {
        "schema_version": SCHEMA_VERSION,
        "created_unix_seconds": int(time.time()),
        "repository": {
            "sha": args.repository_sha,
            "tracked_inputs_match_candidate": True,
            "allowed_generated_diff": GENERATED_STATUS_PATH,
            "status_porcelain": status_lines,
        },
        "run": {
            "id": str(args.run_id),
            "attempt": str(args.run_attempt),
            "operating_system": platform.system(),
            "architecture": platform.machine(),
        },
        "binary": {
            "path": relative_path(root, binary_path),
            "sha256": binary_sha256,
            "version_output": binary_version,
            "transport": "stdio",
        },
        "perl": {"path": str(perl_path), "version": perl_version},
        "fixtures": tracked_records(
            root,
            args.fixture,
            args.repository_sha,
            expected=REQUIRED_FIXTURES,
            context="fixture",
        ),
        "sources": tracked_records(
            root,
            REQUIRED_SOURCE_SUBJECTS,
            args.repository_sha,
            expected=REQUIRED_SOURCE_SUBJECTS,
            context="scorecard source",
        ),
        "raw_receipt": {
            "path": relative_path(root, raw_path),
            "sha256": sha256(raw_path),
        },
        "generated_status": {
            "path": relative_path(root, status_path),
            "sha256": sha256(status_path),
        },
        "scorecard": scorecard,
    }


def validate_packet(args: argparse.Namespace) -> Mapping[str, Any]:
    root = Path(args.repository_root).resolve()
    packet_path = Path(args.packet)
    if not packet_path.is_absolute():
        packet_path = root / packet_path
    packet = as_object(read_json(packet_path), "scorecard packet")
    expect_equal(packet.get("schema_version"), SCHEMA_VERSION, "packet schema")
    verify_head(root, args.expected_repository_sha)
    current_status = assert_repository_state(root)

    repository = as_object(packet.get("repository"), "packet.repository")
    expect_equal(repository.get("sha"), args.expected_repository_sha, "repository SHA")
    expect_equal(
        repository.get("tracked_inputs_match_candidate"), True, "tracked input candidate binding"
    )
    expect_equal(
        repository.get("allowed_generated_diff"), GENERATED_STATUS_PATH, "generated status path"
    )
    expect_equal(repository.get("status_porcelain"), current_status, "repository status")

    run = as_object(packet.get("run"), "packet.run")
    expect_equal(str(run.get("id")), str(args.expected_run_id), "CI run ID")
    expect_equal(str(run.get("attempt")), str(args.expected_run_attempt), "CI run attempt")
    created = as_int(packet.get("created_unix_seconds"), "packet.created_unix_seconds")
    age = int(time.time()) - created
    if age < 0 or age > args.max_age_seconds:
        raise PacketError(
            f"scorecard packet is stale or future-dated: age={age}s, max={args.max_age_seconds}s"
        )

    binary = as_object(packet.get("binary"), "packet.binary")
    validate_subject_hash(root, binary, "candidate binary")
    expect_equal(binary.get("sha256"), args.expected_binary_sha256, "candidate binary SHA-256")
    expect_equal(binary.get("transport"), "stdio", "candidate binary transport")
    binary_path = root / str(binary.get("path"))
    expect_equal(
        binary.get("version_output"),
        run_text([str(binary_path), "--version"]),
        "candidate binary version output",
    )

    perl = as_object(packet.get("perl"), "packet.perl")
    perl_path = perl.get("path")
    if not isinstance(perl_path, str) or not perl_path:
        raise PacketError("packet.perl.path is missing")
    expect_equal(
        perl.get("version"),
        run_text([perl_path, "-e", "print $^V"]),
        "Perl runtime version",
    )

    raw_receipt = as_object(packet.get("raw_receipt"), "packet.raw_receipt")
    validate_subject_hash(root, raw_receipt, "raw scorecard receipt")
    raw_scorecard = validate_scorecard(read_json(root / str(raw_receipt.get("path"))))
    validate_exact_binary_subject(raw_scorecard, binary_path, str(binary.get("sha256")))

    generated_status = as_object(packet.get("generated_status"), "packet.generated_status")
    validate_subject_hash(root, generated_status, "generated DAP status")
    validate_generated_status(root / str(generated_status.get("path")))
    validate_tracked_packet_records(
        root,
        packet.get("fixtures"),
        args.expected_repository_sha,
        REQUIRED_FIXTURES,
        "fixtures",
    )
    validate_tracked_packet_records(
        root,
        packet.get("sources"),
        args.expected_repository_sha,
        REQUIRED_SOURCE_SUBJECTS,
        "sources",
    )
    embedded_scorecard = validate_scorecard(packet.get("scorecard"))
    expect_equal(embedded_scorecard, raw_scorecard, "embedded scorecard")
    return packet


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    build = subparsers.add_parser("build")
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
    validate = subparsers.add_parser("validate")
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
            write_json(Path(args.output), build_packet(args))
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
